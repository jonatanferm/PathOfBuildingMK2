//! Wasm storage backend for the Builds tab.
//!
//! Three layers, in priority order:
//! 1. **Folder mode** — if the user has connected a folder via the File
//!    System Access API (Chromium-only, feature-detected at runtime),
//!    list/save/load go through the directory handle. The handle is
//!    persisted in IDB so it survives reload (with a permission
//!    re-prompt).
//! 2. **IndexedDB** — default. Saves the `.mk2` payload + metadata
//!    directly into an object store, so the list survives reload.
//! 3. **Manual download** — every Save also triggers a browser
//!    download so the user keeps a real file. Load is exposed via
//!    `Import file…`, which opens a file picker and (on success) adds
//!    the build to the IDB list.
//!
//! All async ops post results into an [`StorageEvent`] inbox that the
//! egui frame loop drains each tick — egui itself is synchronous, so
//! this module never blocks rendering.

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Array, Function, Object, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    Blob, BlobPropertyBag, Document, DomException, Event, FileReader, HtmlAnchorElement,
    HtmlInputElement, IdbDatabase, IdbObjectStoreParameters, IdbOpenDbRequest, IdbRequest,
    IdbTransactionMode, Url, Window,
};

use pob_engine::Character;

use crate::builds_tab::{BuildEntry, BuildId, BuildsAction};
use crate::StatusKind;

const DB_NAME: &str = "pob_mk2_builds";
const DB_VERSION: u32 = 1;
const STORE_BUILDS: &str = "builds";
const STORE_META: &str = "meta";
const META_KEY_CATEGORIES: &str = "categories";
const META_KEY_FOLDER_HANDLE: &str = "folder_handle";

/// Event posted by an async storage op for the frame loop to apply.
#[derive(Debug, Clone)]
pub enum StorageEvent {
    /// Listing refreshed. Replaces `builds_state.entries`.
    Refreshed(Vec<BuildEntry>),
    /// Loaded build's payload — host parses + sets character.
    Loaded { label: String, payload: String },
    /// Status toast (info or error).
    Status(StatusKind, String),
    /// Folder mode flipped. Caption / button state updates.
    FolderState {
        connected: bool,
        name: Option<String>,
    },
    /// Browser support for `showDirectoryPicker` resolved at boot.
    FolderSupport(bool),
}

pub struct WasmStorage {
    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    events: Vec<StorageEvent>,
}

impl Default for WasmStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmStorage {
    pub fn new() -> Self {
        let inner = Rc::new(RefCell::new(Inner { events: Vec::new() }));
        let s = Self { inner };
        s.detect_folder_support();
        s.attempt_restore_folder_then_refresh();
        s
    }

    pub fn drain_events(&self) -> Vec<StorageEvent> {
        std::mem::take(&mut self.inner.borrow_mut().events)
    }

    pub fn handle_action(&self, action: BuildsAction, character: &Character) {
        match action {
            BuildsAction::Refresh => self.refresh(),
            BuildsAction::Load(id) => self.load(id),
            BuildsAction::Save { name, category } => match pob_engine::export_code(character) {
                Ok(payload) => self.save(name, "mk2".to_owned(), category, payload),
                Err(e) => self.push(StorageEvent::Status(
                    StatusKind::Error,
                    format!("Save failed: {e}"),
                )),
            },
            BuildsAction::Rename { id, new_label } => self.rename(id, new_label),
            BuildsAction::Duplicate(id) => self.duplicate(id),
            BuildsAction::Delete(id) => self.delete(id),
            BuildsAction::CreateCategory(name) => self.create_category(name),
            BuildsAction::ImportFile => self.import_file(),
            BuildsAction::ConnectFolder => self.connect_folder(),
            BuildsAction::DisconnectFolder => self.disconnect_folder(),
            BuildsAction::OpenFolder => {} // no-op on wasm
        }
    }

    fn push(&self, event: StorageEvent) {
        self.inner.borrow_mut().events.push(event);
    }

    fn detect_folder_support(&self) {
        let supported = window()
            .and_then(|w| Reflect::get(&w, &JsValue::from_str("showDirectoryPicker")).ok())
            .map(|v| v.is_function())
            .unwrap_or(false);
        self.push(StorageEvent::FolderSupport(supported));
    }

    fn attempt_restore_folder_then_refresh(&self) {
        let inner = self.inner.clone();
        spawn_local(async move {
            // Try restoring a previously-connected folder handle. If
            // the browser auto-grants permission, switch to folder
            // mode; otherwise fall back to IDB-only.
            let restored: Option<(JsValue, String)> = match restore_folder_handle().await {
                Ok(Some((handle, name))) => Some((handle, name)),
                _ => None,
            };
            if let Some((_handle, name)) = &restored {
                push_event(
                    &inner,
                    StorageEvent::FolderState {
                        connected: true,
                        name: Some(name.clone()),
                    },
                );
            }
            // Now refresh (uses folder if restored, IDB otherwise).
            match list_entries(restored.as_ref().map(|(h, _)| h.clone())).await {
                Ok(entries) => push_event(&inner, StorageEvent::Refreshed(entries)),
                Err(err) => push_event(
                    &inner,
                    StorageEvent::Status(
                        StatusKind::Error,
                        format!("Storage error: {}", format_err(&err)),
                    ),
                ),
            }
        });
    }

    fn refresh(&self) {
        let inner = self.inner.clone();
        spawn_local(async move {
            let folder = current_folder_handle().await;
            match list_entries(folder).await {
                Ok(entries) => push_event(&inner, StorageEvent::Refreshed(entries)),
                Err(err) => push_event(
                    &inner,
                    StorageEvent::Status(
                        StatusKind::Error,
                        format!("Storage error: {}", format_err(&err)),
                    ),
                ),
            }
        });
    }

    fn load(&self, id: BuildId) {
        let inner = self.inner.clone();
        spawn_local(async move {
            let res = match &id {
                BuildId::Idb(uuid) => idb_load(uuid).await,
                BuildId::Folder(filename) => folder_load(filename).await,
                BuildId::Disk(_) => Err(JsValue::from_str("Disk paths unavailable on wasm")),
            };
            match res {
                Ok((label, payload)) => {
                    push_event(&inner, StorageEvent::Loaded { label, payload });
                }
                Err(err) => push_event(
                    &inner,
                    StorageEvent::Status(
                        StatusKind::Error,
                        format!("Load failed: {}", format_err(&err)),
                    ),
                ),
            }
        });
    }

    fn save(&self, name: String, ext: String, category: Option<String>, payload: String) {
        let inner = self.inner.clone();
        // Always trigger a download regardless of backend so the user
        // ends up with a real file. Best-effort — failures here only
        // surface a status message; the IDB / folder write still
        // proceeds.
        if let Err(err) = trigger_download(&format!("{name}.{ext}"), &payload) {
            push_event(
                &inner,
                StorageEvent::Status(
                    StatusKind::Info,
                    format!("Download skipped: {}", format_err(&err)),
                ),
            );
        }
        let label = name.clone();
        spawn_local(async move {
            let folder = current_folder_handle().await;
            let result = if let Some(handle) = folder {
                folder_save(&handle, &name, &ext, payload.clone()).await
            } else {
                idb_save(&name, &ext, category.as_deref(), &payload).await
            };
            match result {
                Ok(()) => {
                    push_event(
                        &inner,
                        StorageEvent::Status(
                            StatusKind::Info,
                            format!("Saved \"{label}\" — file downloaded + tracked in browser."),
                        ),
                    );
                    refresh_into(&inner).await;
                }
                Err(err) => push_event(
                    &inner,
                    StorageEvent::Status(
                        StatusKind::Error,
                        format!("Save failed: {}", format_err(&err)),
                    ),
                ),
            }
        });
    }

    fn rename(&self, id: BuildId, new_label: String) {
        let inner = self.inner.clone();
        spawn_local(async move {
            let result = match &id {
                BuildId::Idb(uuid) => idb_rename(uuid, &new_label).await,
                BuildId::Folder(filename) => folder_rename(filename, &new_label).await,
                BuildId::Disk(_) => Err(JsValue::from_str("Disk ids unavailable on wasm")),
            };
            match result {
                Ok(()) => {
                    push_event(
                        &inner,
                        StorageEvent::Status(
                            StatusKind::Info,
                            format!("Renamed to \"{new_label}\""),
                        ),
                    );
                    refresh_into(&inner).await;
                }
                Err(err) => push_event(
                    &inner,
                    StorageEvent::Status(
                        StatusKind::Error,
                        format!("Rename failed: {}", format_err(&err)),
                    ),
                ),
            }
        });
    }

    fn duplicate(&self, id: BuildId) {
        let inner = self.inner.clone();
        spawn_local(async move {
            let result = match &id {
                BuildId::Idb(uuid) => idb_duplicate(uuid).await,
                BuildId::Folder(filename) => folder_duplicate(filename).await,
                BuildId::Disk(_) => Err(JsValue::from_str("Disk ids unavailable on wasm")),
            };
            match result {
                Ok(new_label) => {
                    push_event(
                        &inner,
                        StorageEvent::Status(
                            StatusKind::Info,
                            format!("Duplicated as \"{new_label}\""),
                        ),
                    );
                    refresh_into(&inner).await;
                }
                Err(err) => push_event(
                    &inner,
                    StorageEvent::Status(
                        StatusKind::Error,
                        format!("Duplicate failed: {}", format_err(&err)),
                    ),
                ),
            }
        });
    }

    fn delete(&self, id: BuildId) {
        let inner = self.inner.clone();
        spawn_local(async move {
            let result = match &id {
                BuildId::Idb(uuid) => idb_delete(uuid).await,
                BuildId::Folder(filename) => folder_delete(filename).await,
                BuildId::Disk(_) => Err(JsValue::from_str("Disk ids unavailable on wasm")),
            };
            match result {
                Ok(()) => {
                    push_event(
                        &inner,
                        StorageEvent::Status(StatusKind::Info, "Deleted.".to_owned()),
                    );
                    refresh_into(&inner).await;
                }
                Err(err) => push_event(
                    &inner,
                    StorageEvent::Status(
                        StatusKind::Error,
                        format!("Delete failed: {}", format_err(&err)),
                    ),
                ),
            }
        });
    }

    fn create_category(&self, name: String) {
        let inner = self.inner.clone();
        spawn_local(async move {
            match idb_create_category(&name).await {
                Ok(()) => {
                    push_event(
                        &inner,
                        StorageEvent::Status(
                            StatusKind::Info,
                            format!("Created category \"{name}\""),
                        ),
                    );
                    refresh_into(&inner).await;
                }
                Err(err) => push_event(
                    &inner,
                    StorageEvent::Status(
                        StatusKind::Error,
                        format!("Create category failed: {}", format_err(&err)),
                    ),
                ),
            }
        });
    }

    fn import_file(&self) {
        let inner = self.inner.clone();
        spawn_local(async move {
            match pick_text_file().await {
                Ok(Some((filename, contents))) => {
                    let (label, ext) = split_label_ext(&filename);
                    let folder = current_folder_handle().await;
                    let result = if let Some(handle) = folder {
                        folder_save(&handle, &label, &ext, contents.clone()).await
                    } else {
                        idb_save(&label, &ext, None, &contents).await
                    };
                    match result {
                        Ok(()) => {
                            push_event(
                                &inner,
                                StorageEvent::Status(
                                    StatusKind::Info,
                                    format!("Imported \"{label}\""),
                                ),
                            );
                            refresh_into(&inner).await;
                        }
                        Err(err) => push_event(
                            &inner,
                            StorageEvent::Status(
                                StatusKind::Error,
                                format!("Import failed: {}", format_err(&err)),
                            ),
                        ),
                    }
                }
                Ok(None) => {} // user cancelled
                Err(err) => push_event(
                    &inner,
                    StorageEvent::Status(
                        StatusKind::Error,
                        format!("Import failed: {}", format_err(&err)),
                    ),
                ),
            }
        });
    }

    fn connect_folder(&self) {
        let inner = self.inner.clone();
        spawn_local(async move {
            match show_directory_picker().await {
                Ok(handle) => {
                    let name = directory_name(&handle).unwrap_or_else(|| "(folder)".to_owned());
                    if let Err(err) = persist_folder_handle(&handle).await {
                        push_event(
                            &inner,
                            StorageEvent::Status(
                                StatusKind::Info,
                                format!(
                                    "Folder connected, but couldn't persist handle: {}",
                                    format_err(&err)
                                ),
                            ),
                        );
                    }
                    push_event(
                        &inner,
                        StorageEvent::FolderState {
                            connected: true,
                            name: Some(name.clone()),
                        },
                    );
                    push_event(
                        &inner,
                        StorageEvent::Status(
                            StatusKind::Info,
                            format!("Connected folder \"{name}\""),
                        ),
                    );
                    refresh_into(&inner).await;
                }
                Err(err) => {
                    // AbortError when user cancels the picker — silent.
                    if !is_abort_error(&err) {
                        push_event(
                            &inner,
                            StorageEvent::Status(
                                StatusKind::Error,
                                format!("Connect folder failed: {}", format_err(&err)),
                            ),
                        );
                    }
                }
            }
        });
    }

    fn disconnect_folder(&self) {
        let inner = self.inner.clone();
        spawn_local(async move {
            let _ = clear_folder_handle().await;
            push_event(
                &inner,
                StorageEvent::FolderState {
                    connected: false,
                    name: None,
                },
            );
            push_event(
                &inner,
                StorageEvent::Status(StatusKind::Info, "Folder disconnected.".to_owned()),
            );
            refresh_into(&inner).await;
        });
    }
}

fn push_event(inner: &Rc<RefCell<Inner>>, event: StorageEvent) {
    inner.borrow_mut().events.push(event);
}

async fn refresh_into(inner: &Rc<RefCell<Inner>>) {
    let folder = current_folder_handle().await;
    match list_entries(folder).await {
        Ok(entries) => push_event(inner, StorageEvent::Refreshed(entries)),
        Err(err) => push_event(
            inner,
            StorageEvent::Status(
                StatusKind::Error,
                format!("Storage error: {}", format_err(&err)),
            ),
        ),
    }
}

// ---------------------------------------------------------------------------
// IndexedDB helpers
// ---------------------------------------------------------------------------

fn window() -> Option<Window> {
    web_sys::window()
}

fn document() -> Option<Document> {
    window().and_then(|w| w.document())
}

async fn open_db() -> Result<IdbDatabase, JsValue> {
    let factory = window()
        .and_then(|w| w.indexed_db().ok().flatten())
        .ok_or_else(|| JsValue::from_str("IndexedDB unavailable"))?;
    let request: IdbOpenDbRequest = factory.open_with_u32(DB_NAME, DB_VERSION)?;
    let upgrade_cb = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        // `onupgradeneeded` only fires when the db version increases.
        // We hold the schema at v1, so the stores never exist yet
        // when this callback runs — create them unconditionally.
        let target = event.target().expect("target");
        let req: &IdbOpenDbRequest = target.unchecked_ref();
        let result = req.result().expect("result");
        let db: IdbDatabase = result.unchecked_into();
        let params = IdbObjectStoreParameters::new();
        params.set_key_path(&JsValue::from_str("id"));
        let _ = db.create_object_store_with_optional_parameters(STORE_BUILDS, &params);
        let params = IdbObjectStoreParameters::new();
        params.set_key_path(&JsValue::from_str("key"));
        let _ = db.create_object_store_with_optional_parameters(STORE_META, &params);
    });
    request.set_onupgradeneeded(Some(upgrade_cb.as_ref().unchecked_ref()));
    let db_value = request_to_future(request.unchecked_ref()).await?;
    drop(upgrade_cb);
    Ok(db_value.unchecked_into())
}

/// Convert an `IdbRequest`-shaped event source into a future. The
/// returned future resolves with `request.result()` on success or the
/// underlying `DomException` (as `JsValue`) on error.
fn request_to_future(request: &IdbRequest) -> JsFuture {
    let promise = Promise::new(&mut |resolve, reject| {
        let resolve_clone = resolve.clone();
        let reject_clone = reject.clone();
        let req_for_success = request.clone();
        let success = Closure::once_into_js(move |_e: Event| {
            let result = req_for_success.result().unwrap_or(JsValue::UNDEFINED);
            let _ = resolve_clone.call1(&JsValue::NULL, &result);
        });
        let req_for_error = request.clone();
        let error = Closure::once_into_js(move |_e: Event| {
            let err = req_for_error
                .error()
                .ok()
                .flatten()
                .map(JsValue::from)
                .unwrap_or_else(|| JsValue::from_str("IDB error"));
            let _ = reject_clone.call1(&JsValue::NULL, &err);
        });
        request.set_onsuccess(Some(success.unchecked_ref()));
        request.set_onerror(Some(error.unchecked_ref()));
    });
    JsFuture::from(promise)
}

async fn list_entries(folder: Option<JsValue>) -> Result<Vec<BuildEntry>, JsValue> {
    if let Some(handle) = folder {
        folder_list(&handle).await
    } else {
        idb_list().await
    }
}

async fn idb_list() -> Result<Vec<BuildEntry>, JsValue> {
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_BUILDS, IdbTransactionMode::Readonly)?;
    let store = tx.object_store(STORE_BUILDS)?;
    let request = store.get_all()?;
    let result = request_to_future(request.unchecked_ref()).await?;
    let array: Array = result.unchecked_into();
    let mut entries = Vec::with_capacity(array.length() as usize);
    for i in 0..array.length() {
        let obj = array.get(i);
        let id = string_field(&obj, "id").unwrap_or_default();
        let label = string_field(&obj, "label").unwrap_or_default();
        let ext = string_field(&obj, "ext").unwrap_or_else(|| "mk2".into());
        let category = string_field(&obj, "category");
        if id.is_empty() || label.is_empty() {
            continue;
        }
        entries.push(BuildEntry {
            label,
            id: BuildId::Idb(id),
            ext,
            category,
        });
    }
    sort_entries(&mut entries);
    Ok(entries)
}

async fn idb_load(uuid: &str) -> Result<(String, String), JsValue> {
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_BUILDS, IdbTransactionMode::Readonly)?;
    let store = tx.object_store(STORE_BUILDS)?;
    let request = store.get(&JsValue::from_str(uuid))?;
    let result = request_to_future(request.unchecked_ref()).await?;
    if result.is_undefined() || result.is_null() {
        return Err(JsValue::from_str("Build not found"));
    }
    let label = string_field(&result, "label").unwrap_or_default();
    let payload = string_field(&result, "payload").unwrap_or_default();
    Ok((label, payload))
}

async fn idb_save(
    name: &str,
    ext: &str,
    category: Option<&str>,
    payload: &str,
) -> Result<(), JsValue> {
    let db = open_db().await?;
    // Look up an existing record with the same label+category so a
    // re-save overwrites in place rather than producing duplicates.
    let existing_id = idb_find_by_label(&db, name, category).await.ok().flatten();
    let id = existing_id.unwrap_or_else(generate_uuid);

    let tx = db.transaction_with_str_and_mode(STORE_BUILDS, IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(STORE_BUILDS)?;
    let record = Object::new();
    set_str(&record, "id", &id);
    set_str(&record, "label", name);
    set_str(&record, "ext", ext);
    if let Some(cat) = category {
        set_str(&record, "category", cat);
    } else {
        Reflect::set(&record, &JsValue::from_str("category"), &JsValue::NULL)?;
    }
    set_str(&record, "payload", payload);
    Reflect::set(
        &record,
        &JsValue::from_str("saved_at"),
        &JsValue::from_f64(now_ms()),
    )?;
    let request = store.put(&record)?;
    request_to_future(request.unchecked_ref()).await?;
    Ok(())
}

async fn idb_find_by_label(
    db: &IdbDatabase,
    label: &str,
    category: Option<&str>,
) -> Result<Option<String>, JsValue> {
    let tx = db.transaction_with_str_and_mode(STORE_BUILDS, IdbTransactionMode::Readonly)?;
    let store = tx.object_store(STORE_BUILDS)?;
    let request = store.get_all()?;
    let result = request_to_future(request.unchecked_ref()).await?;
    let array: Array = result.unchecked_into();
    for i in 0..array.length() {
        let obj = array.get(i);
        let l = string_field(&obj, "label").unwrap_or_default();
        let c = string_field(&obj, "category");
        if l == label && c.as_deref() == category {
            return Ok(string_field(&obj, "id"));
        }
    }
    Ok(None)
}

async fn idb_rename(uuid: &str, new_label: &str) -> Result<(), JsValue> {
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_BUILDS, IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(STORE_BUILDS)?;
    let request = store.get(&JsValue::from_str(uuid))?;
    let record = request_to_future(request.unchecked_ref()).await?;
    if record.is_undefined() || record.is_null() {
        return Err(JsValue::from_str("Build not found"));
    }
    set_str(&record, "label", new_label);
    let put = store.put(&record)?;
    request_to_future(put.unchecked_ref()).await?;
    Ok(())
}

async fn idb_duplicate(uuid: &str) -> Result<String, JsValue> {
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_BUILDS, IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(STORE_BUILDS)?;
    let get = store.get(&JsValue::from_str(uuid))?;
    let original = request_to_future(get.unchecked_ref()).await?;
    if original.is_undefined() || original.is_null() {
        return Err(JsValue::from_str("Build not found"));
    }
    let label = string_field(&original, "label").unwrap_or_default();
    let new_label = format!("{label} copy");
    let new_id = generate_uuid();
    let copy = Object::new();
    set_str(&copy, "id", &new_id);
    set_str(&copy, "label", &new_label);
    if let Some(ext) = string_field(&original, "ext") {
        set_str(&copy, "ext", &ext);
    }
    let cat = string_field(&original, "category");
    if let Some(cat) = cat {
        set_str(&copy, "category", &cat);
    } else {
        Reflect::set(&copy, &JsValue::from_str("category"), &JsValue::NULL)?;
    }
    if let Some(payload) = string_field(&original, "payload") {
        set_str(&copy, "payload", &payload);
    }
    Reflect::set(
        &copy,
        &JsValue::from_str("saved_at"),
        &JsValue::from_f64(now_ms()),
    )?;
    let put = store.put(&copy)?;
    request_to_future(put.unchecked_ref()).await?;
    Ok(new_label)
}

async fn idb_delete(uuid: &str) -> Result<(), JsValue> {
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_BUILDS, IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(STORE_BUILDS)?;
    let request = store.delete(&JsValue::from_str(uuid))?;
    request_to_future(request.unchecked_ref()).await?;
    Ok(())
}

async fn idb_create_category(name: &str) -> Result<(), JsValue> {
    // Category list is stored as a single meta record under
    // `META_KEY_CATEGORIES`. The list is purely informational — saves
    // can use any category string, listed or not — but we surface it
    // so the UI stays consistent across reloads.
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_META, IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(STORE_META)?;
    let get = store.get(&JsValue::from_str(META_KEY_CATEGORIES))?;
    let existing = request_to_future(get.unchecked_ref()).await?;
    let mut categories: Vec<String> = if existing.is_undefined() || existing.is_null() {
        Vec::new()
    } else {
        let value = Reflect::get(&existing, &JsValue::from_str("value")).unwrap_or(JsValue::NULL);
        if let Ok(array) = value.dyn_into::<Array>() {
            (0..array.length())
                .filter_map(|i| array.get(i).as_string())
                .collect()
        } else {
            Vec::new()
        }
    };
    if !categories.iter().any(|c| c == name) {
        categories.push(name.to_owned());
    }
    let record = Object::new();
    set_str(&record, "key", META_KEY_CATEGORIES);
    let arr = Array::new();
    for c in &categories {
        arr.push(&JsValue::from_str(c));
    }
    Reflect::set(&record, &JsValue::from_str("value"), &arr)?;
    let put = store.put(&record)?;
    request_to_future(put.unchecked_ref()).await?;
    Ok(())
}

async fn persist_folder_handle(handle: &JsValue) -> Result<(), JsValue> {
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_META, IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(STORE_META)?;
    let record = Object::new();
    set_str(&record, "key", META_KEY_FOLDER_HANDLE);
    Reflect::set(&record, &JsValue::from_str("value"), handle)?;
    let put = store.put(&record)?;
    request_to_future(put.unchecked_ref()).await?;
    Ok(())
}

async fn clear_folder_handle() -> Result<(), JsValue> {
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_META, IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(STORE_META)?;
    let req = store.delete(&JsValue::from_str(META_KEY_FOLDER_HANDLE))?;
    request_to_future(req.unchecked_ref()).await?;
    Ok(())
}

async fn restore_folder_handle() -> Result<Option<(JsValue, String)>, JsValue> {
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_META, IdbTransactionMode::Readonly)?;
    let store = tx.object_store(STORE_META)?;
    let req = store.get(&JsValue::from_str(META_KEY_FOLDER_HANDLE))?;
    let record = request_to_future(req.unchecked_ref()).await?;
    if record.is_undefined() || record.is_null() {
        return Ok(None);
    }
    let handle = Reflect::get(&record, &JsValue::from_str("value"))?;
    if handle.is_undefined() || handle.is_null() {
        return Ok(None);
    }
    // Verify we still have permission. Some browsers auto-grant for
    // recently-used handles; others require an explicit user gesture
    // and will reject. We treat rejection as "not connected" rather
    // than surfacing an error.
    if !permission_granted(&handle).await.unwrap_or(false) {
        return Ok(None);
    }
    let name = directory_name(&handle).unwrap_or_else(|| "(folder)".to_owned());
    Ok(Some((handle, name)))
}

async fn current_folder_handle() -> Option<JsValue> {
    match restore_folder_handle().await {
        Ok(Some((handle, _))) => Some(handle),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// File System Access helpers
// ---------------------------------------------------------------------------

async fn show_directory_picker() -> Result<JsValue, JsValue> {
    let win = window().ok_or_else(|| JsValue::from_str("no window"))?;
    let func = Reflect::get(&win, &JsValue::from_str("showDirectoryPicker"))?;
    let func: Function = func.dyn_into()?;
    let opts = Object::new();
    Reflect::set(
        &opts,
        &JsValue::from_str("mode"),
        &JsValue::from_str("readwrite"),
    )?;
    let promise = func.call1(&win, &opts)?;
    let promise: Promise = promise.dyn_into()?;
    JsFuture::from(promise).await
}

async fn permission_granted(handle: &JsValue) -> Result<bool, JsValue> {
    let opts = Object::new();
    Reflect::set(
        &opts,
        &JsValue::from_str("mode"),
        &JsValue::from_str("readwrite"),
    )?;
    let func = Reflect::get(handle, &JsValue::from_str("queryPermission"))?;
    let granted = if let Ok(func) = func.dyn_into::<Function>() {
        let promise = func.call1(handle, &opts)?;
        let result = JsFuture::from(promise.dyn_into::<Promise>()?).await?;
        result.as_string().map(|s| s == "granted").unwrap_or(false)
    } else {
        false
    };
    if granted {
        return Ok(true);
    }
    // Fall back to requestPermission, which is allowed without a user
    // gesture only for already-granted handles in some browsers.
    let func = Reflect::get(handle, &JsValue::from_str("requestPermission"))?;
    if let Ok(func) = func.dyn_into::<Function>() {
        let promise = func.call1(handle, &opts)?;
        let result = JsFuture::from(promise.dyn_into::<Promise>()?).await?;
        Ok(result.as_string().map(|s| s == "granted").unwrap_or(false))
    } else {
        Ok(false)
    }
}

fn directory_name(handle: &JsValue) -> Option<String> {
    Reflect::get(handle, &JsValue::from_str("name"))
        .ok()
        .and_then(|v| v.as_string())
}

async fn folder_list(handle: &JsValue) -> Result<Vec<BuildEntry>, JsValue> {
    let entries_func = Reflect::get(handle, &JsValue::from_str("entries"))?;
    let entries_func: Function = entries_func.dyn_into()?;
    let iter = entries_func.call0(handle)?;
    let next_func: Function = Reflect::get(&iter, &JsValue::from_str("next"))?.dyn_into()?;
    let mut entries = Vec::new();
    loop {
        let promise = next_func.call0(&iter)?;
        let result = JsFuture::from(promise.dyn_into::<Promise>()?).await?;
        let done = Reflect::get(&result, &JsValue::from_str("done"))?
            .as_bool()
            .unwrap_or(true);
        if done {
            break;
        }
        let value = Reflect::get(&result, &JsValue::from_str("value"))?;
        // value is [name, FileSystemHandle]
        let array: Array = value.dyn_into()?;
        let name = array.get(0).as_string().unwrap_or_default();
        let kind = Reflect::get(&array.get(1), &JsValue::from_str("kind"))?
            .as_string()
            .unwrap_or_default();
        if kind != "file" {
            continue;
        }
        let (label, ext) = split_label_ext(&name);
        if ext != "mk2" && ext != "xml" {
            continue;
        }
        entries.push(BuildEntry {
            label,
            id: BuildId::Folder(name),
            ext,
            category: None,
        });
    }
    sort_entries(&mut entries);
    Ok(entries)
}

async fn folder_load(filename: &str) -> Result<(String, String), JsValue> {
    let handle = current_folder_handle()
        .await
        .ok_or_else(|| JsValue::from_str("Folder not connected"))?;
    let get_file = Reflect::get(&handle, &JsValue::from_str("getFileHandle"))?;
    let get_file: Function = get_file.dyn_into()?;
    let promise = get_file.call1(&handle, &JsValue::from_str(filename))?;
    let file_handle = JsFuture::from(promise.dyn_into::<Promise>()?).await?;
    let get_file_method =
        Reflect::get(&file_handle, &JsValue::from_str("getFile"))?.dyn_into::<Function>()?;
    let promise = get_file_method.call0(&file_handle)?;
    let file = JsFuture::from(promise.dyn_into::<Promise>()?).await?;
    let text_method = Reflect::get(&file, &JsValue::from_str("text"))?.dyn_into::<Function>()?;
    let promise = text_method.call0(&file)?;
    let text = JsFuture::from(promise.dyn_into::<Promise>()?).await?;
    let payload = text.as_string().unwrap_or_default();
    let (label, _) = split_label_ext(filename);
    Ok((label, payload))
}

async fn folder_save(
    handle: &JsValue,
    name: &str,
    ext: &str,
    payload: String,
) -> Result<(), JsValue> {
    let filename = format!("{name}.{ext}");
    let opts = Object::new();
    Reflect::set(&opts, &JsValue::from_str("create"), &JsValue::TRUE)?;
    let get_file = Reflect::get(handle, &JsValue::from_str("getFileHandle"))?;
    let get_file: Function = get_file.dyn_into()?;
    let promise = get_file.call2(handle, &JsValue::from_str(&filename), &opts)?;
    let file_handle = JsFuture::from(promise.dyn_into::<Promise>()?).await?;
    let create_writable =
        Reflect::get(&file_handle, &JsValue::from_str("createWritable"))?.dyn_into::<Function>()?;
    let promise = create_writable.call0(&file_handle)?;
    let writable = JsFuture::from(promise.dyn_into::<Promise>()?).await?;
    let write_method =
        Reflect::get(&writable, &JsValue::from_str("write"))?.dyn_into::<Function>()?;
    let promise = write_method.call1(&writable, &JsValue::from_str(&payload))?;
    JsFuture::from(promise.dyn_into::<Promise>()?).await?;
    let close_method =
        Reflect::get(&writable, &JsValue::from_str("close"))?.dyn_into::<Function>()?;
    let promise = close_method.call0(&writable)?;
    JsFuture::from(promise.dyn_into::<Promise>()?).await?;
    Ok(())
}

async fn folder_rename(filename: &str, new_label: &str) -> Result<(), JsValue> {
    let handle = current_folder_handle()
        .await
        .ok_or_else(|| JsValue::from_str("Folder not connected"))?;
    // Read original payload, write under new name, then remove old.
    // The File System Access API doesn't expose a native rename.
    let (_, payload) = folder_load(filename).await?;
    let (_, ext) = split_label_ext(filename);
    folder_save(&handle, new_label, &ext, payload).await?;
    folder_delete(filename).await?;
    Ok(())
}

async fn folder_duplicate(filename: &str) -> Result<String, JsValue> {
    let handle = current_folder_handle()
        .await
        .ok_or_else(|| JsValue::from_str("Folder not connected"))?;
    let (label, ext) = split_label_ext(filename);
    let new_label = format!("{label} copy");
    let (_, payload) = folder_load(filename).await?;
    folder_save(&handle, &new_label, &ext, payload).await?;
    Ok(new_label)
}

async fn folder_delete(filename: &str) -> Result<(), JsValue> {
    let handle = current_folder_handle()
        .await
        .ok_or_else(|| JsValue::from_str("Folder not connected"))?;
    let remove = Reflect::get(&handle, &JsValue::from_str("removeEntry"))?;
    let remove: Function = remove.dyn_into()?;
    let promise = remove.call1(&handle, &JsValue::from_str(filename))?;
    JsFuture::from(promise.dyn_into::<Promise>()?).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Download (Save) + file picker (Load) plumbing
// ---------------------------------------------------------------------------

fn trigger_download(filename: &str, payload: &str) -> Result<(), JsValue> {
    let document = document().ok_or_else(|| JsValue::from_str("no document"))?;
    let array = Array::new();
    array.push(&JsValue::from_str(payload));
    let bag = BlobPropertyBag::new();
    bag.set_type("application/octet-stream");
    let blob = Blob::new_with_str_sequence_and_options(&array, &bag)?;
    let url = Url::create_object_url_with_blob(&blob)?;
    let anchor: HtmlAnchorElement = document.create_element("a")?.dyn_into()?;
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.style().set_property("display", "none")?;
    document
        .body()
        .ok_or_else(|| JsValue::from_str("no body"))?
        .append_child(&anchor)?;
    anchor.click();
    document.body().unwrap().remove_child(&anchor)?;
    Url::revoke_object_url(&url)?;
    Ok(())
}

async fn pick_text_file() -> Result<Option<(String, String)>, JsValue> {
    let document = document().ok_or_else(|| JsValue::from_str("no document"))?;
    let input: HtmlInputElement = document.create_element("input")?.dyn_into()?;
    input.set_type("file");
    input.set_accept(".mk2,.xml");
    input.style().set_property("display", "none")?;
    document
        .body()
        .ok_or_else(|| JsValue::from_str("no body"))?
        .append_child(&input)?;

    let promise = Promise::new(&mut |resolve, reject| {
        let input_for_change = input.clone();
        let resolve_change = resolve.clone();
        let reject_change = reject.clone();
        let on_change = Closure::once_into_js(move |_e: Event| {
            let files = input_for_change.files();
            let Some(files) = files else {
                let _ = resolve_change.call1(&JsValue::NULL, &JsValue::NULL);
                return;
            };
            if files.length() == 0 {
                let _ = resolve_change.call1(&JsValue::NULL, &JsValue::NULL);
                return;
            }
            let file = files.get(0).expect("file");
            let name = file.name();
            let reader = match FileReader::new() {
                Ok(r) => r,
                Err(e) => {
                    let _ = reject_change.call1(&JsValue::NULL, &e);
                    return;
                }
            };
            let resolve_inner = resolve_change.clone();
            let reject_inner = reject_change.clone();
            let reader_for_load = reader.clone();
            let on_load = Closure::once_into_js(move |_e: Event| match reader_for_load.result() {
                Ok(value) => {
                    let text = value.as_string().unwrap_or_default();
                    let result = Array::new();
                    result.push(&JsValue::from_str(&name));
                    result.push(&JsValue::from_str(&text));
                    let _ = resolve_inner.call1(&JsValue::NULL, &result);
                }
                Err(e) => {
                    let _ = reject_inner.call1(&JsValue::NULL, &e);
                }
            });
            reader.set_onload(Some(on_load.unchecked_ref()));
            if let Err(e) = reader.read_as_text(&file) {
                let _ = reject_change.call1(&JsValue::NULL, &e);
            }
        });
        input.set_onchange(Some(on_change.unchecked_ref()));
        input.click();
    });
    let result = JsFuture::from(promise).await;
    // Detach the temporary input regardless of outcome.
    if let Some(parent) = input.parent_node() {
        let _ = parent.remove_child(&input);
    }
    let result = result?;
    if result.is_null() || result.is_undefined() {
        return Ok(None);
    }
    let array: Array = result.dyn_into()?;
    let name = array.get(0).as_string().unwrap_or_default();
    let text = array.get(1).as_string().unwrap_or_default();
    Ok(Some((name, text)))
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn string_field(obj: &JsValue, key: &str) -> Option<String> {
    Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_string())
}

fn set_str(obj: &JsValue, key: &str, value: &str) {
    let _ = Reflect::set(obj, &JsValue::from_str(key), &JsValue::from_str(value));
}

fn split_label_ext(filename: &str) -> (String, String) {
    if let Some(idx) = filename.rfind('.') {
        let (label, dot_ext) = filename.split_at(idx);
        let ext = dot_ext.trim_start_matches('.').to_ascii_lowercase();
        (label.to_owned(), ext)
    } else {
        (filename.to_owned(), "mk2".to_owned())
    }
}

fn sort_entries(entries: &mut [BuildEntry]) {
    entries.sort_by(|a, b| {
        let ca = a.category.as_deref().unwrap_or("");
        let cb = b.category.as_deref().unwrap_or("");
        let by_cat = match (a.category.is_some(), b.category.is_some()) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            _ => ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase()),
        };
        by_cat.then_with(|| {
            a.label
                .to_ascii_lowercase()
                .cmp(&b.label.to_ascii_lowercase())
        })
    });
}

fn now_ms() -> f64 {
    js_sys::Date::now()
}

fn generate_uuid() -> String {
    // Prefer crypto.randomUUID() when available (Chrome 92+, FF 95+,
    // Safari 15.4+). Reflect-based lookup avoids a dependency on
    // web-sys's `Crypto` feature for everyone.
    if let Some(win) = window() {
        if let Ok(crypto) = Reflect::get(&win, &JsValue::from_str("crypto")) {
            if let Ok(func) = Reflect::get(&crypto, &JsValue::from_str("randomUUID")) {
                if let Ok(func) = func.dyn_into::<Function>() {
                    if let Ok(result) = func.call0(&crypto) {
                        if let Some(s) = result.as_string() {
                            return s;
                        }
                    }
                }
            }
        }
    }
    // Fallback: timestamp + Math.random() — not RFC4122 but unique
    // enough for browser-local IDB keys.
    let r = js_sys::Math::random();
    format!(
        "uuid-{:013.0}-{:08x}",
        now_ms(),
        (r * 4_294_967_295.0) as u32
    )
}

fn format_err(err: &JsValue) -> String {
    if let Some(s) = err.as_string() {
        return s;
    }
    if let Some(exc) = err.dyn_ref::<DomException>() {
        let name = exc.name();
        let msg = exc.message();
        if name == "QuotaExceededError" {
            return format!(
                "browser storage quota exceeded ({msg}). Try deleting old saves \
                 or connecting a folder."
            );
        }
        return format!("{name}: {msg}");
    }
    if let Ok(value) = Reflect::get(err, &JsValue::from_str("message")) {
        if let Some(s) = value.as_string() {
            return s;
        }
    }
    format!("{err:?}")
}

fn is_abort_error(err: &JsValue) -> bool {
    err.dyn_ref::<DomException>()
        .map(|e| e.name() == "AbortError")
        .unwrap_or(false)
}

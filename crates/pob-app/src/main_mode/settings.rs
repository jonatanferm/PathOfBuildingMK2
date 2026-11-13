//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1-864 (LoadSettings/SaveSettings).
//! Owner: U16.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! `main:LoadSettings` (:520-681), `main:LoadSharedItems` (:683-736), `main:SaveSettings`
//! (:738-817). The XML text is read and written through `cx.sys.fs` (wasm rule, ui doc §3) with
//! the calc plan's `runtime/lua/xml.lua` port (`model::xml`, calc:BM-A) for parsing and
//! composing; [`load_xml_file`] / [`save_xml_file`] are `xml.LoadXMLFile` / `xml.SaveXMLFile`
//! (xml.lua:97-110, :173-185) over `Fs`.
//!
//! LUA-ORDER: SaveSettings builds its `attrib` tables with constructors and iterates
//! `gameAccounts` and `sharedItemSet.slots` with `pairs`; xml.lua writes attributes in `pairs`
//! order. PoB's string-key order is random per process (calc doc D8), so its Settings.xml
//! attribute order is not reproducible; the port writes attributes in the constructor's source
//! order and the two maps in insertion order (T4 compares after attribute-order normalisation,
//! ui doc §11.1).

use indexmap::IndexMap;
use pob_lua::FmtArg;
use pob_ui_core::prelude::*;

use super::{tostring_opt_bool, tostring_opt_num, GameAccount, ModeArg, SharedItemSet};
use crate::app::CxApp;
use crate::launch;
use crate::model::{self, LStr, XmlChild, XmlNode};

/// The message part of a C library error (`strerror`), as LuaJIT's `io.open` reports it.
pub fn strerror(e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        return "No such file or directory".to_owned();
    }
    let s = e.to_string();
    match s.find(" (os error ") {
        Some(i) => s[..i].to_owned(),
        None => s,
    }
}

/// `xml.LoadXMLFile(fileName)` over `cx.sys.fs`: the pseudo-root (`stack[1]`, whose children are
/// the top-level nodes) or `nil, errMsg`.
/// lua: ../runtime/lua/xml.lua:97-110 (over Fs)
pub fn load_xml_file(cx: &Cx, file_name: &str) -> Result<XmlNode, String> {
    // L98-101 io.open(fileName, "r"): nil, "<name>: <strerror>"
    let bytes = cx
        .sys
        .fs
        .read(file_name)
        .map_err(|e| format!("{file_name}: {}", strerror(&e)))?;
    // L102 read("*a") (bytes; the port's strings are UTF-8, invalid bytes become U+FFFD)
    let file_text = String::from_utf8_lossy(&bytes).into_owned();
    // L104-108 (`not fileText` cannot happen once the file is open)
    if file_text.is_empty() {
        return Err(format!("{file_name} file is empty"));
    }
    // L109
    model::xml::ParseXML_root(&file_text)
}

/// `xml.SaveXMLFile(rootNode, fileName)` over `cx.sys.fs`: `true` or `nil, errMsg`.
/// lua: ../runtime/lua/xml.lua:173-185 (over Fs)
pub fn save_xml_file(cx: &mut Cx, root: &XmlNode, file_name: &str) -> Result<(), String> {
    // L174 (composeNode cannot fail on a typed tree)
    let text = model::xml::ComposeXML(root);
    // L178-184 io.open(fileName, "w+"), write, close
    cx.sys
        .fs
        .write(file_name, text.as_bytes())
        .map_err(|e| format!("{file_name}: {}", strerror(&e)))
}

fn attr<'a>(node: &'a XmlNode, k: &str) -> Option<&'a str> {
    node.attrib.get(k).map(|v| &**v)
}

fn attr_s(node: &XmlNode, k: &str) -> Option<String> {
    attr(node, k).map(str::to_owned)
}

/// `node.attrib.k == "true"` (only called when the attribute is present).
fn attr_true(node: &XmlNode, k: &str) -> bool {
    attr(node, k) == Some("true")
}

/// `setXML[1]` of a parsed file: its first top-level child (`elem` nil when it is text).
fn first_child(root: &XmlNode) -> Option<&XmlChild> {
    root.children.first()
}

/// The shared head of LoadSettings and LoadSharedItems (L521-539 / L684-702): the
/// `<PathOfBuilding>` element, or `Err(ret)` with the function's early return.
fn read_settings_root(cx: &mut Cx) -> Result<XmlNode, bool> {
    let user_path = cx
        .app()
        .main
        .user_path
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to concatenate field 'userPath' (a nil value)"));
    let file = format!("{user_path}Settings.xml");
    // L524
    let (set_xml, err_msg) = match load_xml_file(cx, &file) {
        Ok(r) => (Some(r), None),
        Err(e) => (None, Some(e)),
    };
    if let Some(err_msg) = &err_msg {
        // L525
        if pob_lua::lmatch(err_msg, ".*file returns nil", None).is_some() {
            // L526-528
            cx.app_mut().main.error_reading_settings = true;
            open_cloud_error_popup(cx, &file);
            return Err(true);
        } else if pob_lua::lmatch(err_msg, ".*No such file or directory", None).is_none() {
            // L529-532
            cx.app_mut().main.error_reading_settings = true;
            launch::show_err_msg(cx, &format!("^1{err_msg}"), &[]);
            return Err(true);
        }
    }
    // L534
    let Some(set_xml) = set_xml else {
        // L535
        return Err(true);
    };
    // L536 setXML[1].elem ~= "PathOfBuilding"
    match first_child(&set_xml) {
        None => lua_error!("attempt to index field '?' (a nil value)"),
        Some(XmlChild::Node(n)) if &*n.elem == "PathOfBuilding" => Ok(n.clone()),
        Some(_) => {
            // L537-538
            launch::show_err_msg(
                cx,
                "^1Error parsing 'Settings.xml': 'PathOfBuilding' root element missing",
                &[],
            );
            Err(true)
        }
    }
}

/// `self:OpenCloudErrorPopup(fileName)` (U14's port needs the `launch` fields, CCR-0084).
fn open_cloud_error_popup(cx: &mut Cx, file_name: &str) {
    let info = launch::launch_info(cx);
    pob_ui_core::popups::open_cloud_error_popup(cx, file_name, &info);
}

/// lua: Main.lua:520-681 main:LoadSettings(ignoreBuild): `true` where Lua returns true.
/// @pob Modules/Main.lua:520-681 h=a855a083
pub fn load_settings(cx: &mut Cx, ignore_build: bool) -> bool {
    // L521
    if cx.app().main.error_reading_settings {
        // L522
        return true;
    }
    // L524-539
    let root = match read_settings_root(cx) {
        Ok(r) => r,
        Err(ret) => return ret,
    };
    // L540
    for node in &root.children {
        // L541
        let XmlChild::Node(node) = node else { continue };
        // L542
        if !ignore_build && &*node.elem == "Mode" {
            // L543
            let mode = attr(node, "mode");
            if !mode.is_some_and(|m| cx.app().main.modes.contains_key(m)) {
                // L544-545
                launch::show_err_msg(
                    cx,
                    "^1Error parsing 'Settings.xml': Invalid mode attribute in 'Mode' element",
                    &[],
                );
                return true;
            }
            // L547
            let mut args: Vec<ModeArg> = Vec::new();
            // L548
            for child in &node.children {
                // L549
                let XmlChild::Node(child) = child else {
                    continue;
                };
                // L550
                if &*child.elem == "Arg" {
                    // L551
                    if let Some(number) = attr(child, "number") {
                        // L552 t_insert(args, tonumber(..)) (a nil appends nothing)
                        if let Some(v) = pob_lua::tonumber(number) {
                            args.push(ModeArg::Num(v));
                        }
                    } else if let Some(s) = attr(child, "string") {
                        // L554
                        args.push(ModeArg::Str(s.to_owned()));
                    } else if let Some(b) = attr(child, "boolean") {
                        // L556
                        args.push(ModeArg::Bool(b == "true"));
                    }
                }
            }
            // L561
            let mode = mode.unwrap_or_default().to_owned();
            super::on_frame::set_mode(cx, &mode, args);
        } else if &*node.elem == "Accounts" {
            let m = &mut cx.app_mut().main;
            // L563-567
            m.last_account_name = attr_s(node, "lastAccountName");
            m.last_realm = attr_s(node, "lastRealm");
            m.last_league = attr_s(node, "lastLeague");
            m.last_token = attr_s(node, "lastToken");
            m.last_refresh_token = attr_s(node, "lastRefreshToken");
            // L568
            m.token_expiry = attr(node, "tokenExpiry").and_then(pob_lua::tonumber);
            // L569
            for child in &node.children {
                // L570
                let XmlChild::Node(child) = child else {
                    continue;
                };
                if &*child.elem == "Account" {
                    // L571-573 self.gameAccounts[child.attrib.accountName] = { .. } (a nil key
                    // is a Lua error)
                    let name = attr_s(child, "accountName")
                        .unwrap_or_else(|| lua_error!("table index is nil"));
                    m.game_accounts.insert(
                        name,
                        GameAccount {
                            session_id: attr_s(child, "sessionID"),
                        },
                    );
                }
            }
        } else if &*node.elem == "Misc" {
            load_misc(cx, node);
        }
    }
    false
}

/// The `elseif node.elem == "Misc"` arm of LoadSettings (L576-678).
fn load_misc(cx: &mut Cx, node: &XmlNode) {
    // L577
    if let Some(v) = attr_s(node, "buildSortMode") {
        // L578
        cx.app_mut().main.build_sort_mode = v;
    }
    // L580
    cx.app_mut().launch.connection_protocol =
        attr(node, "connectionProtocol").and_then(pob_lua::tonumber);
    // L581
    cx.app_mut().launch.proxy_url = attr_s(node, "proxyURL");
    // L582
    if let Some(v) = attr_s(node, "buildPath") {
        // L583
        cx.app_mut().main.build_path = Some(v);
    }
    // L585
    if let Some(v) = attr_s(node, "nodePowerTheme") {
        // L586
        cx.app_mut().main.node_power_theme = v;
    }
    // L588
    if let Some(v) = attr_s(node, "colorPositive") {
        // L589
        cx.main.cc.update_color_code("POSITIVE", &v);
        // L590
        cx.app_mut().main.color_positive = v;
    }
    // L592
    if let Some(v) = attr_s(node, "colorNegative") {
        // L593
        cx.main.cc.update_color_code("NEGATIVE", &v);
        // L594
        cx.app_mut().main.color_negative = v;
    }
    // L596
    if let Some(v) = attr_s(node, "colorHighlight") {
        // L597
        if let Some(rgb) = cx.main.cc.update_color_code("HIGHLIGHT", &v) {
            cx.app_mut().main.rgb_color = rgb;
        }
        // L598
        cx.app_mut().main.color_highlight = v;
    }

    // L601-602 In order to preserve users' settings through renaming/merging this variable, we have this if statement to use the first found setting
    // Once the user has closed PoB once, they will be using the new `showThousandsSeparator` variable name, so after some time, this statement may be removed
    // L603
    if attr(node, "showThousandsCalcs").is_some() {
        // L604
        cx.main.show_thousands_separators = attr_true(node, "showThousandsCalcs");
    } else if attr(node, "showThousandsSidebar").is_some() {
        // L606
        cx.main.show_thousands_separators = attr_true(node, "showThousandsSidebar");
    }
    // L608
    if attr(node, "showThousandsSeparators").is_some() {
        // L609
        cx.main.show_thousands_separators = attr_true(node, "showThousandsSeparators");
    }
    // L611
    if let Some(v) = attr_s(node, "thousandsSeparator") {
        // L612
        cx.main.thousands_separator = v;
    }
    // L614
    if attr(node, "useCompactValues").is_some() {
        // L615
        cx.app_mut().main.use_compact_values = attr_true(node, "useCompactValues");
    }
    // L617
    if let Some(v) = attr_s(node, "decimalSeparator") {
        // L618
        cx.main.decimal_separator = v;
    }
    let m = &mut cx.app_mut().main;
    // L620
    if attr(node, "showTitlebarName").is_some() {
        // L621
        m.show_titlebar_name = attr_true(node, "showTitlebarName");
    }
    // L623
    if attr(node, "betaTest").is_some() {
        // L624
        m.beta_test = Some(attr_true(node, "betaTest"));
    }
    // L626
    if attr(node, "edgeSearchHighlight").is_some() {
        // L627
        m.edge_search_highlight = attr_true(node, "edgeSearchHighlight");
    }
    // L629
    if let Some(v) = attr(node, "defaultGemQuality") {
        // L630
        m.default_gem_quality = Some(pob_lua::math::m_min(
            pob_lua::tonumber(v).unwrap_or(0.0),
            23.0,
        ));
    }
    // L632
    if let Some(v) = attr(node, "defaultCharLevel") {
        // L633
        m.default_char_level = Some(pob_lua::math::m_min(
            pob_lua::math::m_max(pob_lua::tonumber(v).unwrap_or(1.0), 1.0),
            100.0,
        ));
    }
    // L635
    if let Some(v) = attr(node, "defaultItemAffixQuality") {
        // L636
        m.default_item_affix_quality = Some(pob_lua::math::m_min(
            pob_lua::tonumber(v).unwrap_or(0.5),
            1.0,
        ));
    }
    // L638
    if let Some(v) = attr_s(node, "lastExportedWebsite") {
        // L639
        m.last_exported_website = Some(v);
    }
    // L641
    if attr(node, "showWarnings").is_some() {
        // L642
        m.show_warnings = attr_true(node, "showWarnings");
    }
    // L644
    if attr(node, "slotOnlyTooltips").is_some() {
        // L645
        m.slot_only_tooltips = attr_true(node, "slotOnlyTooltips");
    }
    // L647
    if attr(node, "migrateEldritchImplicits").is_some() {
        // L648
        m.migrate_eldritch_implicits = attr_true(node, "migrateEldritchImplicits");
    }
    // L650
    if attr(node, "notSupportedModTooltips").is_some() {
        // L651
        cx.main.not_supported_mod_tooltips = attr_true(node, "notSupportedModTooltips");
    }
    let m = &mut cx.app_mut().main;
    // L653
    if attr(node, "invertSliderScrollDirection").is_some() {
        // L654
        m.invert_slider_scroll_direction = Some(attr_true(node, "invertSliderScrollDirection"));
    }
    // L656
    if attr(node, "disableDevAutoSave").is_some() {
        // L657
        m.disable_dev_auto_save = Some(attr_true(node, "disableDevAutoSave"));
    }
    // L659
    if attr(node, "showPublicBuilds").is_some() {
        // L660
        m.show_public_builds = attr_true(node, "showPublicBuilds");
    }
    // L662
    if attr(node, "showFlavourText").is_some() {
        // L663
        cx.main.show_flavour_text = attr_true(node, "showFlavourText");
    }
    let m = &mut cx.app_mut().main;
    // L665
    if attr(node, "showAnimations").is_some() {
        // L666
        m.show_animations = attr_true(node, "showAnimations");
    }
    // L668
    if attr(node, "showAllItemAffixes").is_some() {
        // L669
        m.show_all_item_affixes = attr_true(node, "showAllItemAffixes");
    }
    // L671
    if attr(node, "disableScrollControlInteraction").is_some() {
        // L672
        cx.main.disable_scroll_control_interaction =
            attr_true(node, "disableScrollControlInteraction");
    }
    // L674
    if let Some(v) = attr(node, "dpiScaleOverridePercent") {
        // L675
        let pct = pob_lua::tonumber(v).unwrap_or(0.0);
        cx.app_mut().main.dpi_scale_override_percent = pct;
        // L676 SetDPIScaleOverridePercent(self.dpiScaleOverridePercent) (SG takes an int)
        cx.sg.set_dpi_scale_override_percent(pct as i32);
    }
}

/// The raw text of an `<Item>` element: its last string child (L708-713 / L720-725).
fn item_raw(node: &XmlNode) -> String {
    let mut raw = String::new();
    for sub_child in &node.children {
        if let XmlChild::Text(t) = sub_child {
            raw = t.to_string();
        }
    }
    raw
}

/// lua: Main.lua:683-736 main:LoadSharedItems()
/// @pob Modules/Main.lua:683-736 h=4216df0e
pub fn load_shared_items(cx: &mut Cx) -> bool {
    // L684
    if cx.app().main.error_reading_settings {
        // L685
        return true;
    }
    // L687-702
    let root = match read_settings_root(cx) {
        Ok(r) => r,
        Err(ret) => return ret,
    };
    // L703
    for node in &root.children {
        // L704
        let XmlChild::Node(node) = node else { continue };
        // L705
        if &*node.elem == "SharedItems" {
            // L706
            for child in &node.children {
                let XmlChild::Node(child) = child else {
                    continue;
                };
                // L707
                if &*child.elem == "Item" {
                    // L708-713
                    let raw = item_raw(child);
                    // L714
                    let new_item = super::new_item(Some(&raw), None, false);
                    // L715
                    cx.app_mut().main.shared_item_list.push(new_item);
                } else if &*child.elem == "ItemSet" {
                    // L717
                    let mut shared_item_set = SharedItemSet {
                        title: attr_s(child, "title"),
                        slots: IndexMap::new(),
                    };
                    // L718
                    for grand_child in &child.children {
                        let XmlChild::Node(grand_child) = grand_child else {
                            continue;
                        };
                        // L719
                        if &*grand_child.elem == "Item" {
                            // L720-725
                            let raw = item_raw(grand_child);
                            // L726
                            let new_item = super::new_item(Some(&raw), None, false);
                            // L727 (a nil slotName is a Lua error)
                            let slot = attr_s(grand_child, "slotName")
                                .unwrap_or_else(|| lua_error!("table index is nil"));
                            shared_item_set.slots.insert(slot, new_item);
                        }
                    }
                    // L730
                    cx.app_mut().main.shared_item_set_list.push(shared_item_set);
                }
            }
        }
    }
    false
}

/// An element `{ elem = name, attrib = { .. } }`, nil attributes left out.
fn elem(name: &str, attrib: &[(&str, Option<String>)]) -> XmlNode {
    let mut node = XmlNode {
        elem: LStr::from(name),
        ..XmlNode::default()
    };
    for (k, v) in attrib {
        if let Some(v) = v {
            node.attrib.insert(LStr::from(*k), LStr::from(v.as_str()));
        }
    }
    node
}

fn s(v: &str) -> Option<String> {
    Some(v.to_owned())
}

fn b(v: bool) -> Option<String> {
    Some(v.to_string())
}

/// The Settings.xml tree SaveSettings writes (L742-811), or `None` after the invalid-mode error.
pub fn compose_settings(cx: &mut Cx) -> Option<XmlNode> {
    // L742
    let mut set_xml = elem("PathOfBuilding", &[]);
    // L743
    let mode_name = cx.app().main.mode.clone();
    let mut mode = elem("Mode", &[("mode", mode_name.clone())]);
    // L744 for _, val in ipairs({ self:CallMode("GetArgs") }) do
    let args = super::on_frame::current_mode(cx)
        .and_then(|m| m.get_args(cx))
        .unwrap_or_default();
    for val in args.iter().take_while(|v| !v.is_nil()) {
        // L745-752
        let child = match val {
            ModeArg::Num(_) => elem("Arg", &[("number", Some(val.tostring()))]),
            ModeArg::Bool(_) => elem("Arg", &[("boolean", Some(val.tostring()))]),
            _ => elem("Arg", &[("string", Some(val.tostring()))]),
        };
        // L753
        mode.children.push(XmlChild::Node(child));
    }

    // L756-757 if setting save is attempted and mode is nil something has gone very wrong
    if mode_name.is_none() || mode.children.is_empty() {
        // L758-759
        launch::show_err_msg(
            cx,
            "^1Error saving 'Settings.xml': mode element is invalid",
            &[],
        );
        return None;
    }
    // L761
    set_xml.children.push(XmlChild::Node(mode));
    let m = &cx.app().main;
    // L762
    let mut accounts = elem(
        "Accounts",
        &[
            ("lastAccountName", m.last_account_name.clone()),
            ("lastRealm", m.last_realm.clone()),
            ("lastLeague", m.last_league.clone()),
            ("lastToken", m.last_token.clone()),
            ("lastRefreshToken", m.last_refresh_token.clone()),
            ("tokenExpiry", Some(tostring_opt_num(m.token_expiry))),
        ],
    );
    // L763 LUA-ORDER: pairs(self.gameAccounts), insertion order
    for (account_name, account) in &m.game_accounts {
        // L764
        accounts.children.push(XmlChild::Node(elem(
            "Account",
            &[
                ("accountName", Some(account_name.clone())),
                ("sessionID", account.session_id.clone()),
            ],
        )));
    }
    // L766
    set_xml.children.push(XmlChild::Node(accounts));
    // L767
    let mut shared_item_list = elem("SharedItems", &[]);
    // L768
    for ver_item in &m.shared_item_list {
        // L769
        let mut item = elem("Item", &[]);
        item.children.push(XmlChild::Text(ver_item.raw.get()));
        shared_item_list.children.push(XmlChild::Node(item));
    }
    // L771
    for shared_item_set in &m.shared_item_set_list {
        // L772
        let mut set = elem("ItemSet", &[("title", shared_item_set.title.clone())]);
        // L773 LUA-ORDER: pairs(sharedItemSet.slots), insertion order
        for (slot_name, ver_item) in &shared_item_set.slots {
            // L774
            let mut item = elem("Item", &[("slotName", Some(slot_name.clone()))]);
            item.children.push(XmlChild::Text(ver_item.raw.get()));
            set.children.push(XmlChild::Node(item));
        }
        // L776
        shared_item_list.children.push(XmlChild::Node(set));
    }
    // L778
    set_xml.children.push(XmlChild::Node(shared_item_list));
    // L779-811
    let l = &cx.app().launch;
    let mc = &cx.main;
    let misc = elem(
        "Misc",
        &[
            ("buildSortMode", s(&m.build_sort_mode)),
            (
                "connectionProtocol",
                Some(tostring_opt_num(l.connection_protocol)),
            ),
            ("proxyURL", l.proxy_url.clone()),
            (
                "buildPath",
                if m.build_path != m.default_build_path {
                    m.build_path.clone()
                } else {
                    None
                },
            ),
            ("nodePowerTheme", s(&m.node_power_theme)),
            ("colorPositive", s(&m.color_positive)),
            ("colorNegative", s(&m.color_negative)),
            ("colorHighlight", s(&m.color_highlight)),
            ("showThousandsSeparators", b(mc.show_thousands_separators)),
            ("thousandsSeparator", s(&mc.thousands_separator)),
            ("useCompactValues", b(m.use_compact_values)),
            ("decimalSeparator", s(&mc.decimal_separator)),
            ("showTitlebarName", b(m.show_titlebar_name)),
            ("betaTest", Some(tostring_opt_bool(m.beta_test))),
            ("edgeSearchHighlight", b(m.edge_search_highlight)),
            (
                "defaultGemQuality",
                Some(pob_lua::tostring_num(m.default_gem_quality.unwrap_or(0.0))),
            ),
            (
                "defaultCharLevel",
                Some(pob_lua::tostring_num(m.default_char_level.unwrap_or(1.0))),
            ),
            (
                "defaultItemAffixQuality",
                Some(pob_lua::tostring_num(
                    m.default_item_affix_quality.unwrap_or(0.5),
                )),
            ),
            ("lastExportedWebsite", m.last_exported_website.clone()),
            ("showWarnings", b(m.show_warnings)),
            ("slotOnlyTooltips", b(m.slot_only_tooltips)),
            ("migrateEldritchImplicits", b(m.migrate_eldritch_implicits)),
            ("notSupportedModTooltips", b(mc.not_supported_mod_tooltips)),
            (
                "invertSliderScrollDirection",
                Some(tostring_opt_bool(m.invert_slider_scroll_direction)),
            ),
            (
                "disableDevAutoSave",
                Some(tostring_opt_bool(m.disable_dev_auto_save)),
            ),
            ("showPublicBuilds", b(m.show_public_builds)),
            ("showFlavourText", b(mc.show_flavour_text)),
            ("showAnimations", b(m.show_animations)),
            ("showAllItemAffixes", b(m.show_all_item_affixes)),
            (
                "disableScrollControlInteraction",
                b(mc.disable_scroll_control_interaction),
            ),
            (
                "dpiScaleOverridePercent",
                Some(pob_lua::tostring_num(m.dpi_scale_override_percent)),
            ),
        ],
    );
    set_xml.children.push(XmlChild::Node(misc));
    Some(set_xml)
}

/// lua: Main.lua:738-817 main:SaveSettings(): `true` where Lua returns true.
/// @pob Modules/Main.lua:738-817 h=b67bf7c8
pub fn save_settings(cx: &mut Cx) -> bool {
    // L739
    if cx.app().main.error_reading_settings {
        // L740
        return false;
    }
    // L742-811
    let Some(set_xml) = compose_settings(cx) else {
        return true;
    };
    // L812
    let user_path = cx
        .app()
        .main
        .user_path
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to concatenate field 'userPath' (a nil value)"));
    let res = save_xml_file(cx, &set_xml, &format!("{user_path}Settings.xml"));
    // L813
    if let Err(err_msg) = res {
        // L814
        launch::show_err_msg(
            cx,
            "Error saving 'Settings.xml': %s",
            &[FmtArg::from(err_msg.as_str())],
        );
        // L815
        return true;
    }
    false
}

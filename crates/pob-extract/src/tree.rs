use std::path::Path;

use ahash::HashMap;
use anyhow::{anyhow, bail, Context, Result};
use pob_data::{
    Ascendancy, Class, Group, GroupBackground, MasteryEffect, Node, NodeId, NodeKind, PassiveTree,
    Rect, TreeConstants, TreePoints, ROOT_NODE_ID,
};
use serde_json::Value;
use smallvec::SmallVec;

use crate::lua_value as lv;
use crate::{load_lua_file_returning, make_lua};

#[derive(Debug)]
pub enum ExtractError {
    /// Tree is in a legacy schema that we don't support (pre-3.0). Skip with a warning.
    IncompatibleSchema(String),
    Other(anyhow::Error),
}

impl From<anyhow::Error> for ExtractError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e)
    }
}

pub fn list_tree_versions(pob_root: &Path) -> Result<Vec<String>> {
    let dir = pob_root.join("src/TreeData");
    let mut versions = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().join("tree.lua").is_file() {
            versions.push(name);
        }
    }
    versions.sort();
    Ok(versions)
}

pub fn extract(pob_root: &Path, version: &str) -> Result<PassiveTree, ExtractError> {
    let path = pob_root.join("src/TreeData").join(version).join("tree.lua");
    let lua = make_lua().map_err(ExtractError::Other)?;
    let value = load_lua_file_returning(&lua, &path).map_err(ExtractError::Other)?;
    let obj = value
        .as_object()
        .ok_or_else(|| ExtractError::Other(anyhow!("tree.lua did not return a table")))?;
    if !obj.contains_key("classes") || !obj.contains_key("groups") || !obj.contains_key("nodes") {
        return Err(ExtractError::IncompatibleSchema(
            "missing one of: classes, groups, nodes (legacy schema)".into(),
        ));
    }
    // Modern format has classes as an array of objects with "name" / "base_str" / etc.
    // Legacy 2_6 has classes as a map keyed by class index. Detect by checking the first
    // element shape if it's an array.
    let classes_ok = match &obj["classes"] {
        Value::Array(a) => a.iter().all(|c| {
            c.as_object()
                .is_some_and(|o| o.contains_key("name") && o.contains_key("base_str"))
        }),
        _ => false,
    };
    if !classes_ok {
        return Err(ExtractError::IncompatibleSchema(
            "classes is not in modern array-of-objects form (legacy schema)".into(),
        ));
    }
    parse_tree(version, &value).map_err(ExtractError::Other)
}

/// Iterate (key_string, value) over either a JSON object or a JSON array (where the
/// array index, 1-based, becomes the key — matching Lua's convention).
fn iter_keyed<'a>(v: &'a Value) -> Box<dyn Iterator<Item = (String, &'a Value)> + 'a> {
    match v {
        Value::Object(m) => Box::new(m.iter().map(|(k, v)| (k.clone(), v))),
        Value::Array(a) => Box::new(a.iter().enumerate().map(|(i, v)| ((i + 1).to_string(), v))),
        _ => Box::new(std::iter::empty()),
    }
}

fn parse_tree(version: &str, v: &Value) -> Result<PassiveTree> {
    let tree_name = lv::opt_str(v, "tree").unwrap_or_else(|| "Default".to_owned());

    let classes = parse_classes(lv::opt_array(v, "classes")?)?;
    let groups = parse_groups(lv::get(v, "groups").ok_or_else(|| anyhow!("missing `groups`"))?)?;
    let nodes = parse_nodes(lv::get(v, "nodes").ok_or_else(|| anyhow!("missing `nodes`"))?)?;
    let jewel_slots = lv::opt_array(v, "jewelSlots")?
        .iter()
        .filter_map(|n| n.as_u64().map(|n| n as NodeId))
        .collect();

    let constants =
        parse_constants(lv::opt_object(v, "constants").context("missing `constants`")?)?;
    let points = lv::opt_object(v, "points")
        .map(|o| {
            let host = Value::Object(o.clone());
            TreePoints {
                total_points: lv::opt_u64(&host, "totalPoints").unwrap_or(123) as u32,
                ascendancy_points: lv::opt_u64(&host, "ascendancyPoints").unwrap_or(8) as u32,
            }
        })
        .unwrap_or_default();

    Ok(PassiveTree {
        version: version.to_owned(),
        tree: tree_name,
        classes,
        groups,
        nodes,
        jewel_slots,
        min_x: lv::opt_i64(v, "min_x").unwrap_or(0) as i32,
        min_y: lv::opt_i64(v, "min_y").unwrap_or(0) as i32,
        max_x: lv::opt_i64(v, "max_x").unwrap_or(0) as i32,
        max_y: lv::opt_i64(v, "max_y").unwrap_or(0) as i32,
        constants,
        points,
    })
}

fn parse_classes(arr: &[Value]) -> Result<Vec<Class>> {
    arr.iter()
        .map(|v| {
            let ascendancies = lv::opt_array(v, "ascendancies")?
                .iter()
                .map(parse_ascendancy)
                .collect::<Result<Vec<_>>>()?;
            Ok(Class {
                name: lv::req_str(v, "name")?,
                base_str: lv::opt_i64(v, "base_str").unwrap_or(0) as i32,
                base_dex: lv::opt_i64(v, "base_dex").unwrap_or(0) as i32,
                base_int: lv::opt_i64(v, "base_int").unwrap_or(0) as i32,
                ascendancies,
            })
        })
        .collect()
}

fn parse_ascendancy(v: &Value) -> Result<Ascendancy> {
    let rect = lv::opt_object(v, "flavourTextRect").map(|o| {
        let host = Value::Object(o.clone());
        Rect {
            x: lv::opt_f64(&host, "x").unwrap_or(0.0) as f32,
            y: lv::opt_f64(&host, "y").unwrap_or(0.0) as f32,
            width: lv::opt_f64(&host, "width").unwrap_or(0.0) as f32,
            height: lv::opt_f64(&host, "height").unwrap_or(0.0) as f32,
        }
    });
    Ok(Ascendancy {
        id: lv::req_str(v, "id")?,
        name: lv::req_str(v, "name")?,
        flavour_text: lv::opt_str(v, "flavourText"),
        flavour_text_colour: lv::opt_str(v, "flavourTextColour"),
        flavour_text_rect: rect,
    })
}

fn parse_groups(host: &Value) -> Result<HashMap<u32, Group>> {
    let mut out: HashMap<u32, Group> = HashMap::default();
    for (k, v) in iter_keyed(host) {
        let id = k
            .parse::<u32>()
            .with_context(|| format!("group id `{k}`"))?;
        let nodes = lv::opt_array(v, "nodes")?
            .iter()
            .map(|nv| {
                if let Some(s) = nv.as_str() {
                    s.parse::<NodeId>()
                        .map_err(|e| anyhow!("group node id `{s}`: {e}"))
                } else if let Some(n) = nv.as_u64() {
                    Ok(n as NodeId)
                } else {
                    bail!("group node id of unexpected type: {nv}")
                }
            })
            .collect::<Result<Vec<NodeId>>>()?;
        let orbits = lv::opt_array(v, "orbits")?
            .iter()
            .filter_map(|n| n.as_u64().map(|n| n as u8))
            .collect::<SmallVec<[u8; 4]>>();
        let background = lv::opt_object(v, "background").map(|o| {
            let host = Value::Object(o.clone());
            GroupBackground {
                image: lv::opt_str(&host, "image").unwrap_or_default(),
                is_half_image: lv::opt_bool(&host, "isHalfImage").unwrap_or(false),
                offset_x: lv::opt_f64(&host, "offsetX").map(|n| n as f32),
                offset_y: lv::opt_f64(&host, "offsetY").map(|n| n as f32),
            }
        });
        out.insert(
            id,
            Group {
                x: lv::opt_f64(v, "x").unwrap_or(0.0) as f32,
                y: lv::opt_f64(v, "y").unwrap_or(0.0) as f32,
                orbits,
                background,
                nodes,
                is_proxy: lv::opt_bool(v, "isProxy").unwrap_or(false),
            },
        );
    }
    Ok(out)
}

fn parse_nodes(host: &Value) -> Result<HashMap<NodeId, Node>> {
    let mut out: HashMap<NodeId, Node> = HashMap::default();
    for (k, v) in iter_keyed(host) {
        let id = if k == "root" {
            ROOT_NODE_ID
        } else {
            k.parse::<NodeId>()
                .with_context(|| format!("node id `{k}`"))?
        };
        out.insert(id, parse_node(id, k == "root", v)?);
    }
    Ok(out)
}

fn parse_node(id: NodeId, is_root: bool, v: &Value) -> Result<Node> {
    let name = lv::opt_str(v, "name");
    let icon = lv::opt_str(v, "icon");
    let ascendancy_name = lv::opt_str(v, "ascendancyName");
    let stats = lv::opt_array(v, "stats")?
        .iter()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect();
    let reminder_text = lv::opt_array(v, "reminderText")?
        .iter()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect();
    let class_start_index = lv::opt_u64(v, "classStartIndex").map(|n| n as u32);
    let group = lv::opt_u64(v, "group").map(|n| n as u32);
    let orbit = lv::opt_u64(v, "orbit").map(|n| n as u8);
    let orbit_index = lv::opt_u64(v, "orbitIndex").map(|n| n as u32);
    let out_edges = parse_edges(v, "out")?;
    let in_edges = parse_edges(v, "in")?;
    let mastery_effects = parse_mastery_effects(v)?;

    let kind = classify_node(is_root, v, &mastery_effects, class_start_index);

    Ok(Node {
        id,
        name,
        icon,
        ascendancy_name,
        stats,
        reminder_text,
        kind,
        class_start_index,
        group,
        orbit,
        orbit_index,
        out_edges,
        in_edges,
        mastery_effects,
        expansion_jewel_size: lv::opt_object(v, "expansionJewel").and_then(|o| {
            let host = Value::Object(o.clone());
            lv::opt_u64(&host, "size").map(|n| n as u8)
        }),
        jewel_radius: lv::opt_u64(v, "jewelRadius").map(|n| n as u8),
    })
}

fn parse_edges(v: &Value, key: &str) -> Result<SmallVec<[NodeId; 4]>> {
    Ok(lv::opt_array(v, key)?
        .iter()
        .filter_map(|nv| {
            if let Some(s) = nv.as_str() {
                s.parse::<NodeId>().ok()
            } else {
                nv.as_u64().map(|n| n as NodeId)
            }
        })
        .collect())
}

fn parse_mastery_effects(v: &Value) -> Result<Vec<MasteryEffect>> {
    let Some(arr) = v
        .as_object()
        .and_then(|m| m.get("masteryEffects"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(arr.len());
    for e in arr {
        let stats = lv::opt_array(e, "stats")?
            .iter()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect();
        let reminder_text = lv::opt_array(e, "reminderText")?
            .iter()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect();
        out.push(MasteryEffect {
            effect: lv::opt_u64(e, "effect").unwrap_or(0) as u32,
            stats,
            reminder_text,
        });
    }
    Ok(out)
}

fn classify_node(
    is_root: bool,
    v: &Value,
    mastery_effects: &[MasteryEffect],
    class_start_index: Option<u32>,
) -> NodeKind {
    if is_root {
        return NodeKind::Root;
    }
    if lv::opt_bool(v, "isAscendancyStart").unwrap_or(false) {
        return NodeKind::AscendancyStart;
    }
    if class_start_index.is_some() {
        return NodeKind::ClassStart;
    }
    if lv::opt_bool(v, "isJewelSocket").unwrap_or(false) {
        return NodeKind::JewelSocket;
    }
    if lv::opt_bool(v, "isMastery").unwrap_or(false) || !mastery_effects.is_empty() {
        return NodeKind::Mastery;
    }
    if lv::opt_bool(v, "isKeystone").unwrap_or(false) {
        return NodeKind::Keystone;
    }
    if lv::opt_bool(v, "isNotable").unwrap_or(false) {
        return NodeKind::Notable;
    }
    if lv::opt_bool(v, "isBlighted").unwrap_or(false) {
        return NodeKind::Blighted;
    }
    if lv::opt_bool(v, "isTattoo").unwrap_or(false) {
        return NodeKind::Tattoo;
    }
    NodeKind::Normal
}

fn parse_constants(o: &serde_json::Map<String, Value>) -> Result<TreeConstants> {
    let host = Value::Object(o.clone());
    let skills_per_orbit = lv::opt_array(&host, "skillsPerOrbit")?
        .iter()
        .filter_map(|n| n.as_u64().map(|n| n as u32))
        .collect();
    let orbit_radii = lv::opt_array(&host, "orbitRadii")?
        .iter()
        .filter_map(|n| n.as_i64().map(|n| n as i32))
        .collect();
    let classes = lv::opt_object(&host, "classes")
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n as u8)))
                .collect()
        })
        .unwrap_or_default();
    let character_attributes = lv::opt_object(&host, "characterAttributes")
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n as u8)))
                .collect()
        })
        .unwrap_or_default();
    Ok(TreeConstants {
        skills_per_orbit,
        orbit_radii,
        classes,
        character_attributes,
        pss_centre_inner_radius: lv::opt_i64(&host, "PSSCentreInnerRadius").map(|n| n as i32),
    })
}

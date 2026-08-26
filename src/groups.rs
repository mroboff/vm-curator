//! User-defined VM groups.
//!
//! A group is just a name plus a set of VM ids ([`crate::vm::DiscoveredVm::id`]).
//! Groups are unrelated to the OS-family hierarchy in [`crate::metadata::hierarchy`]
//! (which is derived automatically from OS metadata) — these are freeform, entirely
//! user-managed, and persisted as a single ordered list in
//! `~/.config/vm-curator/groups.toml`. List order is significant: it's the order
//! the Groups screen displays them in, and the user controls it directly.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A user-defined group of VMs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VmGroup {
    pub name: String,
    /// VM ids belonging to this group.
    #[serde(default)]
    pub vm_ids: Vec<String>,
}

impl VmGroup {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            vm_ids: Vec::new(),
        }
    }

    pub fn contains(&self, vm_id: &str) -> bool {
        self.vm_ids.iter().any(|id| id == vm_id)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct GroupsFile {
    #[serde(default)]
    groups: Vec<VmGroup>,
}

/// Path to the groups definition file.
pub fn groups_file_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("vm-curator")
        .join("groups.toml")
}

/// Load groups from the given file path, in file order. Returns an empty list
/// if the file doesn't exist or fails to parse.
pub fn load_groups(path: &Path) -> Vec<VmGroup> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    toml::from_str::<GroupsFile>(&content)
        .map(|f| f.groups)
        .unwrap_or_default()
}

/// Save groups, in the given order, to the given file path — creating parent
/// directories as needed.
pub fn save_groups(path: &Path, groups: &[VmGroup]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {:?}", parent))?;
    }
    let file = GroupsFile {
        groups: groups.to_vec(),
    };
    let content = toml::to_string_pretty(&file).context("Failed to serialize groups")?;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write groups to {:?}", path))?;
    Ok(())
}

/// Remove membership entries referencing a VM id not in `valid_ids` (e.g. a VM
/// that was deleted, or whose library directory disappeared out from under
/// the app). Returns true if any group's membership actually changed.
pub fn prune_stale(groups: &mut [VmGroup], valid_ids: &HashSet<&str>) -> bool {
    let mut changed = false;
    for group in groups.iter_mut() {
        let before = group.vm_ids.len();
        group.vm_ids.retain(|id| valid_ids.contains(id.as_str()));
        if group.vm_ids.len() != before {
            changed = true;
        }
    }
    changed
}

/// A VM's default group is its OS-family category (the same categorization
/// the automatic hierarchy view uses) — this assigns each `(vm_id,
/// category_name)` pair to the matching group, creating that group if it
/// doesn't exist yet (positioned among the other category groups using
/// `category_order`, and after any group that isn't a category at all).
/// A VM already in any group (however it got there) is left alone — this
/// only ever supplies a *default*, never overrides a user's own choice.
/// Returns true if anything changed.
pub fn assign_to_category_groups(
    groups: &mut Vec<VmGroup>,
    vm_categories: &[(String, String)],
    category_order: &HashMap<String, i32>,
) -> bool {
    let mut changed = false;
    for (vm_id, category_name) in vm_categories {
        if groups.iter().any(|g| g.contains(vm_id)) {
            continue;
        }

        let group_idx = match groups.iter().position(|g| &g.name == category_name) {
            Some(i) => i,
            None => {
                let order = category_order
                    .get(category_name)
                    .copied()
                    .unwrap_or(i32::MAX);
                let insert_at = groups
                    .iter()
                    .position(|g| category_order.get(&g.name).copied().unwrap_or(i32::MAX) > order)
                    .unwrap_or(groups.len());
                groups.insert(insert_at, VmGroup::new(category_name.clone()));
                insert_at
            }
        };
        groups[group_idx].vm_ids.push(vm_id.clone());
        changed = true;
    }
    changed
}

#[cfg(test)]
#[path = "tests/groups.rs"]
mod tests;

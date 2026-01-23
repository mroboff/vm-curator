use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget},
};

use crate::app::App;
use crate::metadata::{HierarchyConfig, MetadataStore, SortBy};
use crate::vm::DiscoveredVm;
use std::collections::BTreeMap;

/// Build the visual order of VMs based on hierarchy (used for navigation)
/// Returns a Vec where index is visual position and value is filtered_idx
pub fn build_visual_order(
    vms: &[DiscoveredVm],
    filtered_indices: &[usize],
    hierarchy: &HierarchyConfig,
    metadata: &MetadataStore,
) -> Vec<usize> {
    let vm_hierarchy = build_vm_hierarchy(vms, filtered_indices, hierarchy, metadata);
    let mut order = Vec::new();

    for family in &hierarchy.families {
        if let Some(subcats) = vm_hierarchy.get(&family.id) {
            for subcat in hierarchy.subcategories_for_family(&family.id) {
                if let Some(vm_entries) = subcats.get(&subcat.id) {
                    for entry in vm_entries {
                        order.push(entry.filtered_idx);
                    }
                }
            }
        }
    }

    order
}

/// Map a clicked row index to the corresponding visual_order index
/// Returns None if the clicked row is a header (not selectable)
pub fn click_row_to_visual_index(
    vms: &[DiscoveredVm],
    filtered_indices: &[usize],
    hierarchy: &HierarchyConfig,
    metadata: &MetadataStore,
    visual_order: &[usize],
    clicked_row: usize,
) -> Option<usize> {
    let vm_hierarchy = build_vm_hierarchy(vms, filtered_indices, hierarchy, metadata);

    // Build index_map to map row -> filtered_idx (None for headers)
    let mut index_map: Vec<Option<usize>> = Vec::new();

    for family in &hierarchy.families {
        if let Some(subcats) = vm_hierarchy.get(&family.id) {
            // Family header
            index_map.push(None);

            let family_subcats: Vec<_> = hierarchy.subcategories_for_family(&family.id);

            for subcat in family_subcats {
                if let Some(vm_entries) = subcats.get(&subcat.id) {
                    // Subcategory header
                    index_map.push(None);

                    // VM entries
                    for entry in vm_entries {
                        index_map.push(Some(entry.filtered_idx));
                    }
                }
            }
        }
    }

    // Get the filtered_idx for the clicked row
    let filtered_idx = index_map.get(clicked_row)?.as_ref()?;

    // Find this filtered_idx's position in visual_order
    visual_order.iter().position(|&idx| idx == *filtered_idx)
}

/// VM list widget state with hierarchical display
pub struct VmListWidget<'a> {
    pub vms: &'a [DiscoveredVm],
    pub filtered_indices: &'a [usize],
    pub visual_order: &'a [usize],
    pub selected: usize,
    pub hierarchy: &'a HierarchyConfig,
    pub metadata: &'a crate::metadata::MetadataStore,
}

impl<'a> VmListWidget<'a> {
    pub fn new(app: &'a App) -> Self {
        Self {
            vms: &app.vms,
            filtered_indices: &app.filtered_indices,
            visual_order: &app.visual_order,
            selected: app.selected_vm,
            hierarchy: &app.hierarchy,
            metadata: &app.metadata,
        }
    }

    pub fn render(self, area: Rect, buf: &mut Buffer) {
        let title = format!(" VMs ({}) ", self.filtered_indices.len());

        // Build hierarchical structure
        let vm_hierarchy = build_vm_hierarchy(self.vms, self.filtered_indices, self.hierarchy, self.metadata);

        // Render as tree with proper indices
        let (items, index_map) = render_hierarchy_items(&vm_hierarchy, self.hierarchy, self.metadata);

        // Get the filtered_idx for the currently selected visual position
        let selected_filtered_idx = self.visual_order.get(self.selected).copied();

        // Find the selected item's position in the rendered list
        let selected_pos = index_map.iter()
            .position(|&idx| idx == selected_filtered_idx)
            .unwrap_or(0);

        let mut state = ListState::default();
        state.select(Some(selected_pos));

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray),
            )
            .highlight_symbol("→ ");

        StatefulWidget::render(list, area, buf, &mut state);
    }
}

/// VM entry with its filtered index
struct VmEntry<'a> {
    vm: &'a DiscoveredVm,
    filtered_idx: usize,
}

/// Build hierarchical structure from VMs
/// Returns: family_id -> subcategory_id -> Vec<VmEntry>
/// VMs within each subcategory are sorted by release_date (oldest first)
fn build_vm_hierarchy<'a>(
    vms: &'a [DiscoveredVm],
    filtered_indices: &[usize],
    hierarchy: &HierarchyConfig,
    metadata: &MetadataStore,
) -> BTreeMap<String, BTreeMap<String, Vec<VmEntry<'a>>>> {
    let mut result: BTreeMap<String, BTreeMap<String, Vec<VmEntry>>> = BTreeMap::new();

    for (filtered_idx, &vm_idx) in filtered_indices.iter().enumerate() {
        let vm = &vms[vm_idx];
        let (family_id, subcat_id) = hierarchy.categorize(&vm.id);

        result
            .entry(family_id)
            .or_default()
            .entry(subcat_id)
            .or_default()
            .push(VmEntry { vm, filtered_idx });
    }

    // Sort VMs within each subcategory based on subcategory's sort_by setting
    for (subcat_id, vm_entries) in result.values_mut().flat_map(|subcats| subcats.iter_mut()) {
        let sort_by = hierarchy
            .get_subcategory(subcat_id)
            .map(|s| s.sort_by)
            .unwrap_or(SortBy::Name);

        vm_entries.sort_by(|a, b| {
            match sort_by {
                SortBy::Date => {
                    // Sort by release date (oldest first), falling back to name
                    let date_a = metadata.get(&a.vm.id).map(|i| i.release_date.as_str()).unwrap_or("");
                    let date_b = metadata.get(&b.vm.id).map(|i| i.release_date.as_str()).unwrap_or("");

                    match (date_a.is_empty(), date_b.is_empty()) {
                        (true, true) => {
                            let name_a = get_display_name(a.vm, metadata);
                            let name_b = get_display_name(b.vm, metadata);
                            name_a.cmp(&name_b)
                        }
                        (true, false) => std::cmp::Ordering::Greater,
                        (false, true) => std::cmp::Ordering::Less,
                        (false, false) => date_a.cmp(date_b),
                    }
                }
                SortBy::Name => {
                    // Sort alphabetically by display name
                    let name_a = get_display_name(a.vm, metadata);
                    let name_b = get_display_name(b.vm, metadata);
                    name_a.cmp(&name_b)
                }
            }
        });
    }

    result
}

/// Get display name for a VM, using metadata if available
fn get_display_name(vm: &DiscoveredVm, metadata: &crate::metadata::MetadataStore) -> String {
    // First try to get display_name from metadata
    if let Some(info) = metadata.get(&vm.id) {
        if let Some(ref display_name) = info.display_name {
            return display_name.clone();
        }
        // Fall back to name field
        if !info.name.is_empty() {
            return info.name.clone();
        }
    }
    // Fall back to VM's own display_name method
    vm.display_name()
}

/// Render hierarchy as list items with tree characters
fn render_hierarchy_items<'a>(
    vm_hierarchy: &BTreeMap<String, BTreeMap<String, Vec<VmEntry<'a>>>>,
    hierarchy: &'a HierarchyConfig,
    metadata: &crate::metadata::MetadataStore,
) -> (Vec<ListItem<'a>>, Vec<Option<usize>>) {
    let mut items = Vec::new();
    let mut index_map: Vec<Option<usize>> = Vec::new();

    // Iterate families in order
    for family in &hierarchy.families {
        if let Some(subcats) = vm_hierarchy.get(&family.id) {
            // Family header with icon
            items.push(ListItem::new(Line::from(vec![
                Span::raw(format!("{} ", family.icon)),
                Span::styled(
                    &family.name,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
            ])));
            index_map.push(None); // Headers are not selectable

            // Get subcategories for this family in order
            let family_subcats: Vec<_> = hierarchy.subcategories_for_family(&family.id);
            let subcat_count = family_subcats.iter()
                .filter(|s| subcats.contains_key(&s.id))
                .count();
            let mut subcat_rendered = 0;

            for subcat in family_subcats {
                if let Some(vm_entries) = subcats.get(&subcat.id) {
                    subcat_rendered += 1;
                    let is_last_subcat = subcat_rendered == subcat_count;
                    let subcat_branch = if is_last_subcat { "└─" } else { "├─" };

                    // Subcategory header
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("  {} {}", subcat_branch, subcat.name),
                            Style::default().fg(Color::Magenta),
                        ),
                    ])));
                    index_map.push(None); // Headers are not selectable

                    let vm_count = vm_entries.len();

                    for (vm_idx, entry) in vm_entries.iter().enumerate() {
                        let is_last_vm = vm_idx == vm_count - 1;
                        let subcat_cont = if is_last_subcat { "  " } else { "│ " };
                        let vm_branch = if is_last_vm { "└─" } else { "├─" };

                        // Get display name from metadata
                        let display_name = get_display_name(entry.vm, metadata);

                        items.push(ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("  {}{} ", subcat_cont, vm_branch),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled(
                                display_name,
                                Style::default().fg(Color::White),
                            ),
                        ])));
                        index_map.push(Some(entry.filtered_idx));
                    }
                }
            }
        }
    }

    (items, index_map)
}

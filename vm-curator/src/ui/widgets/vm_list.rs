use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::app::App;
use crate::vm::DiscoveredVm;

/// VM list widget state
pub struct VmListWidget<'a> {
    pub vms: &'a [DiscoveredVm],
    pub filtered_indices: &'a [usize],
    pub selected: usize,
}

impl<'a> VmListWidget<'a> {
    pub fn new(app: &'a App) -> Self {
        Self {
            vms: &app.vms,
            filtered_indices: &app.filtered_indices,
            selected: app.selected_vm,
        }
    }

    pub fn render(self, area: Rect, buf: &mut Buffer) {
        let title = format!(" VMs ({}) ", self.filtered_indices.len());

        let items: Vec<ListItem> = self
            .filtered_indices
            .iter()
            .map(|&idx| {
                let vm = &self.vms[idx];
                ListItem::new(vm.display_name())
            })
            .collect();

        let mut state = ListState::default();
        state.select(Some(self.selected));

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray),
            )
            .highlight_symbol("> ");

        StatefulWidget::render(list, area, buf, &mut state);
    }
}

/// Render grouped VM list with category headers
pub fn render_grouped_vm_list(
    groups: &[(&str, Vec<&DiscoveredVm>)],
    selected_id: Option<&str>,
    area: Rect,
    buf: &mut Buffer,
) {
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected_index = None;
    let mut current_index = 0;

    for (category, vms) in groups {
        // Add category header
        items.push(
            ListItem::new(Line::from(vec![Span::styled(
                format!("─── {} ───", category),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]))
        );
        current_index += 1;

        // Add VMs
        for vm in vms {
            if selected_id == Some(&vm.id) {
                selected_index = Some(current_index);
            }
            items.push(ListItem::new(format!("  {}", vm.display_name())));
            current_index += 1;
        }
    }

    let mut state = ListState::default();
    state.select(selected_index);

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Virtual Machines ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray),
        )
        .highlight_symbol("> ");

    StatefulWidget::render(list, area, buf, &mut state);
}

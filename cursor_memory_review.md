# VM Curator – Cursor Memory Review

## Summary: **Reasonable total memory, but a lot of unnecessary allocation**

For a TUI with a VM library, overall memory use is likely fine. The main cost is **allocation churn**: many short‑lived allocations in hot paths (every frame, every keystroke).

---

## What's already reasonable

- **`visual_order`** is `Vec<usize>` — cheap.
- **No `Arc`/`Rc`** — fine for a single‑threaded TUI; you're not paying for refcounting.
- **Metadata** (profiles, hierarchy, etc.) is loaded once at startup and kept in `App`.
- **Background work** correctly sends owned data (clones) across threads.

---

## Main sources of churn

### 1. `build_vm_hierarchy` every frame

The VM list widget builds a full `BTreeMap<…, BTreeMap<…, Vec<VmEntry>>>` on **every render**:

```rust
// vm-curator/src/ui/widgets/vm_list.rs (around line 106)
let vm_hierarchy = build_vm_hierarchy(self.vms, self.filtered_indices, self.hierarchy, self.metadata);
```

That rebuilds the whole hierarchy and all the `VmEntry` collections each frame. Same hierarchy is also rebuilt for `click_row_to_visual_index`.

### 2. `display_name()` / `get_display_name` return `String`

- `DiscoveredVm::display_name()` allocates a new `String` every call.
- `get_display_name` often clones from metadata or falls back to `display_name()`.
- Both are used heavily: VM list, main menu, config, management, search, etc. Sorting in `build_vm_hierarchy` calls `get_display_name` for each comparison, so you get multiple allocations per VM per frame.

### 3. Search / filter on every keystroke

```rust
// vm-curator/src/app.rs (around lines 666–684)
pub fn update_filter(&mut self) {
    // ...
    self.filtered_indices = (0..self.vms.len()).collect();  // or filter + collect
    // ...
    self.visual_order = build_visual_order(...);  // full rebuild
}
```

Each keystroke:

- Allocates a new `filtered_indices` and `visual_order`.
- Uses `vm.display_name().to_lowercase().contains(&query)` — two allocations per VM per keystroke.

### 4. Cloning large state

- **`push_screen`**: clones the entire `Screen` (which can hold `ConfirmAction` with `String`s) each time you navigate.
- **Wizard**: `CreateWizardState` (and `WizardQemuConfig`) are cloned when starting the create task; both are `String`/`Vec` heavy.
- **Profile → config**: `WizardQemuConfig::from_profile` clones every `String`/`Vec` from the profile.

### 5. Script editor

```rust
// vm-curator/src/app.rs (around lines 922–925)
self.script_editor_lines = vm.config.raw_script.lines().map(String::from).collect();
```

The whole script is copied into a `Vec<String>`. Fine for editing, but it doubles script memory while the editor is open.

### 6. `selected_vm_info()`

Returns `Option<OsInfo>` by cloning from metadata (or constructing a default). Called when showing VM details, so you clone `OsInfo` (with its `String`s and `Vec`s) whenever the UI needs it.

---

## Quick wins (biggest impact first)

| Change | Benefit |
|--------|--------|
| **Cache `build_vm_hierarchy` result** | Reuse when `vms`, `filtered_indices`, `hierarchy`, or `metadata` haven't changed. Avoid rebuilding every frame. |
| **`display_name` as `&str`** | Cache a display name (e.g. in `DiscoveredVm` or alongside it) or return `Cow<str>`, and use `&str` in UI. Cuts many per‑frame allocations. |
| **Reuse `filtered_indices` / `visual_order` in search** | Retain buffers and update in place, or only reallocate when the query actually changes. |
| **Lighter `Screen` stack** | Store minimal data in `Screen` (e.g. indices or small enums); resolve details from `App` when rendering. Avoid cloning `String`s in `ConfirmAction` where you can. |
| **`selected_vm_info` by reference** | Return `Option<&OsInfo>` (or similar) instead of `Option<OsInfo>` where the UI only reads; clone only when you really need an owned value. |

---

## Verdict

- **Peak memory**: Likely fine for tens/hundreds of VMs; no red flags.
- **Efficiency**: The hot paths (VM list render, search, navigation, wizard) do more allocation and work than necessary. Reducing **clones** and **per‑frame / per‑keystroke allocations** (especially around `build_vm_hierarchy`, `display_name`/`get_display_name`, and filter updates) would make the app more memory‑ and CPU‑friendly without changing behavior.

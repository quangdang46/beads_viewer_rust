//! Keybinding registry — documents all key bindings per focus context.
//! Feeds shortcuts sidebar, help overlay, and --robot-help.
//! Runtime dispatch is in App::handle_key; this registry is for documentation only.
//! Port of Go `pkg/ui/keybindings.go`.

use std::collections::BTreeMap;

/// Focus context for key bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Focus {
    List,
    Detail,
    Board,
    Tree,
    Graph,
    Insights,
    Alerts,
    FlowMatrix,
    Attention,
    Sprint,
    History,
    Tutorial,
    Search,
}

/// A single key binding document.
#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub focus: Focus,
    pub key: String,
    pub desc: String,
    pub category: String,
}

/// Registry of all key bindings, indexed by focus.
pub struct KeyRegistry {
    bindings: BTreeMap<Focus, Vec<KeyBinding>>,
}

impl Default for KeyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyRegistry {
    pub fn new() -> Self {
        KeyRegistry {
            bindings: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, binding: KeyBinding) {
        self.bindings
            .entry(binding.focus)
            .or_default()
            .push(binding);
    }

    pub fn bindings_for(&self, focus: Focus) -> &[KeyBinding] {
        self.bindings.get(&focus).map_or(&[], |v| v.as_slice())
    }

    pub fn all_bindings(&self) -> &BTreeMap<Focus, Vec<KeyBinding>> {
        &self.bindings
    }
}

/// Build the default key registry with all documented bindings.
pub fn build_default_registry() -> KeyRegistry {
    let mut reg = KeyRegistry::new();
    let nav = "Navigation".to_string();
    let act = "Actions".to_string();
    let view = "Views".to_string();

    // List view bindings
    for (key, desc) in [
        ("j/↓", "Move down"),
        ("k/↑", "Move up"),
        ("enter", "Toggle detail pane"),
        ("tab", "Focus detail pane"),
        ("/", "Search"),
        ("a", "Show all issues"),
        ("o", "Show open issues"),
        ("c", "Show closed issues"),
        ("r", "Show ready issues"),
        ("s", "Cycle sort mode"),
        ("S", "Triage sort (priority)"),
        ("L", "Cycle label filter"),
        ("w", "Cycle workspace repo filter"),
    ] {
        reg.register(KeyBinding {
            focus: Focus::List,
            key: key.to_string(),
            desc: desc.to_string(),
            category: nav.clone(),
        });
    }

    // View toggle bindings
    for (key, desc) in [
        ("b", "Toggle board view"),
        ("E", "Toggle tree view"),
        ("G", "Toggle graph view"),
        ("i", "Toggle insights view"),
        ("f", "Toggle flow-matrix view"),
        ("A", "Toggle attention view"),
        ("!", "Toggle alerts view"),
        ("t", "Toggle time-travel/history view"),
        ("P", "Toggle sprint view"),
        ("`", "Toggle tutorial"),
        (";", "Toggle sidebar"),
    ] {
        reg.register(KeyBinding {
            focus: Focus::List,
            key: key.to_string(),
            desc: desc.to_string(),
            category: view.clone(),
        });
    }

    // Action bindings
    for (key, desc) in [
        ("?", "Show help"),
        ("x", "Export markdown report"),
        ("C", "Copy issue to clipboard"),
        ("O", "Open issue in $EDITOR"),
        ("Ctrl+R", "Refresh from disk"),
        ("q", "Quit / close view"),
    ] {
        reg.register(KeyBinding {
            focus: Focus::List,
            key: key.to_string(),
            desc: desc.to_string(),
            category: act.clone(),
        });
    }

    // Detail view bindings
    for (key, desc) in [
        ("j/↓", "Scroll down"),
        ("k/↑", "Scroll up"),
        ("tab", "Return to list"),
        ("esc", "Return to list"),
    ] {
        reg.register(KeyBinding {
            focus: Focus::Detail,
            key: key.to_string(),
            desc: desc.to_string(),
            category: nav.clone(),
        });
    }

    // Sprint view bindings
    for (key, desc) in [
        ("j/↓", "Switch to next sprint"),
        ("k/↑", "Switch to previous sprint"),
        ("P/esc", "Close sprint view"),
    ] {
        reg.register(KeyBinding {
            focus: Focus::Sprint,
            key: key.to_string(),
            desc: desc.to_string(),
            category: nav.clone(),
        });
    }

    // Graph view bindings
    for (key, desc) in [
        ("j/↓", "Next issue"),
        ("k/↑", "Previous issue"),
        ("G", "Close graph view"),
    ] {
        reg.register(KeyBinding {
            focus: Focus::Graph,
            key: key.to_string(),
            desc: desc.to_string(),
            category: nav.clone(),
        });
    }

    // History view bindings
    for (key, desc) in [
        ("j/↓", "Next bead"),
        ("k/↑", "Previous bead"),
        ("t", "Toggle bead/git mode"),
    ] {
        reg.register(KeyBinding {
            focus: Focus::History,
            key: key.to_string(),
            desc: desc.to_string(),
            category: nav.clone(),
        });
    }

    reg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_list_bindings() {
        let reg = build_default_registry();
        let list = reg.bindings_for(Focus::List);
        assert!(!list.is_empty());
        assert!(list.iter().any(|b| b.key == "j/↓"));
        assert!(list.iter().any(|b| b.key == "q"));
    }

    #[test]
    fn registry_has_all_focuses() {
        let reg = build_default_registry();
        assert!(!reg.bindings_for(Focus::List).is_empty());
        assert!(!reg.bindings_for(Focus::Detail).is_empty());
        assert!(!reg.bindings_for(Focus::Graph).is_empty());
    }
}

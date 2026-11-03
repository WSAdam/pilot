//! The window's tab strip, reduced from `Target.*` events.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub target_id: String,
    pub title: String,
    pub url: String,
}

/// Page targets in creation order, and the one the window shows.
#[derive(Clone, Debug, Default)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<String>,
}

impl Tabs {
    /// Applies one CDP event. Returns whether the tabs changed. A new page
    /// becomes the active tab, so the window follows the agent to it.
    pub fn apply(&mut self, event: &Value) -> bool {
        let info = &event["params"]["targetInfo"];
        match event["method"].as_str() {
            Some("Target.targetCreated") if info["type"] == "page" => {
                let Some(tab) = tab_from(info) else {
                    return false;
                };
                if self.position(&tab.target_id).is_some() {
                    return self.update(tab);
                }
                self.active = Some(tab.target_id.clone());
                self.tabs.push(tab);
                true
            }
            Some("Target.targetInfoChanged") if info["type"] == "page" => {
                tab_from(info).is_some_and(|tab| self.update(tab))
            }
            Some("Target.targetDestroyed") => event["params"]["targetId"]
                .as_str()
                .is_some_and(|target_id| self.remove(target_id)),
            _ => false,
        }
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active(&self) -> Option<&Tab> {
        let active = self.active.as_deref()?;
        self.tabs.iter().find(|tab| tab.target_id == active)
    }

    pub fn active_index(&self) -> Option<usize> {
        self.position(self.active.as_deref()?)
    }

    /// Shows another tab (a click in the strip). Returns whether it changed.
    pub fn activate(&mut self, index: usize) -> bool {
        let Some(tab) = self.tabs.get(index) else {
            return false;
        };
        let changed = self.active.as_deref() != Some(tab.target_id.as_str());
        self.active = Some(tab.target_id.clone());
        changed
    }

    fn position(&self, target_id: &str) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.target_id == target_id)
    }

    fn update(&mut self, tab: Tab) -> bool {
        let Some(index) = self.position(&tab.target_id) else {
            return false;
        };
        let changed = self.tabs[index] != tab;
        self.tabs[index] = tab;
        changed
    }

    fn remove(&mut self, target_id: &str) -> bool {
        let Some(index) = self.position(target_id) else {
            return false;
        };
        self.tabs.remove(index);
        if self.active.as_deref() == Some(target_id) {
            // Like a browser: show the tab that slid into its place, or the
            // new last tab.
            self.active = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.last())
                .map(|tab| tab.target_id.clone());
        }
        true
    }
}

fn tab_from(info: &Value) -> Option<Tab> {
    Some(Tab {
        target_id: info["targetId"].as_str()?.to_owned(),
        title: info["title"].as_str().unwrap_or_default().to_owned(),
        url: info["url"].as_str().unwrap_or_default().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn target_event(method: &str, target_id: &str, kind: &str, url: &str) -> Value {
        json!({"method": method, "params": {"targetInfo": {
            "targetId": target_id, "type": kind, "title": url, "url": url
        }}})
    }

    fn destroyed(target_id: &str) -> Value {
        json!({"method": "Target.targetDestroyed", "params": {"targetId": target_id}})
    }

    fn ids(tabs: &Tabs) -> Vec<&str> {
        tabs.tabs()
            .iter()
            .map(|tab| tab.target_id.as_str())
            .collect()
    }

    #[test]
    fn page_targets_become_tabs_and_the_newest_is_shown() {
        let mut tabs = Tabs::default();
        assert!(tabs.apply(&target_event(
            "Target.targetCreated",
            "A",
            "page",
            "about:blank"
        )));
        assert!(!tabs.apply(&target_event(
            "Target.targetCreated",
            "W",
            "service_worker",
            "x"
        )));
        assert!(!tabs.apply(&target_event("Target.targetCreated", "B0", "browser", "")));
        assert!(tabs.apply(&target_event(
            "Target.targetCreated",
            "B",
            "page",
            "https://b.test/"
        )));
        assert_eq!(ids(&tabs), ["A", "B"]);
        assert_eq!(tabs.active().unwrap().target_id, "B");
        assert_eq!(tabs.active_index(), Some(1));
    }

    #[test]
    fn info_changes_update_the_title_and_url() {
        let mut tabs = Tabs::default();
        tabs.apply(&target_event(
            "Target.targetCreated",
            "A",
            "page",
            "about:blank",
        ));
        assert!(tabs.apply(&target_event(
            "Target.targetInfoChanged",
            "A",
            "page",
            "https://a.test/"
        )));
        assert!(!tabs.apply(&target_event(
            "Target.targetInfoChanged",
            "A",
            "page",
            "https://a.test/"
        )));
        assert!(!tabs.apply(&target_event(
            "Target.targetInfoChanged",
            "Z",
            "page",
            "https://z.test/"
        )));
        assert_eq!(tabs.active().unwrap().url, "https://a.test/");
    }

    #[test]
    fn closing_the_shown_tab_shows_its_neighbour() {
        let mut tabs = Tabs::default();
        for id in ["A", "B", "C"] {
            tabs.apply(&target_event("Target.targetCreated", id, "page", id));
        }
        assert!(tabs.activate(1));
        assert!(!tabs.activate(1));
        assert!(!tabs.activate(7));
        assert!(tabs.apply(&destroyed("B")));
        assert_eq!(tabs.active().unwrap().target_id, "C");
        assert!(tabs.apply(&destroyed("C")));
        assert_eq!(tabs.active().unwrap().target_id, "A");
        assert!(!tabs.apply(&destroyed("C")));
        assert!(tabs.apply(&destroyed("A")));
        assert!(tabs.active().is_none());
    }

    #[test]
    fn closing_another_tab_keeps_the_shown_one() {
        let mut tabs = Tabs::default();
        for id in ["A", "B"] {
            tabs.apply(&target_event("Target.targetCreated", id, "page", id));
        }
        tabs.apply(&destroyed("A"));
        assert_eq!(tabs.active().unwrap().target_id, "B");
    }
}

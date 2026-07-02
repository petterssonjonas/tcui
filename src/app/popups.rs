use super::TuiApp;

impl TuiApp {
    pub(crate) fn show_skills_popup(&mut self) {
        let items = crate::skills::SkillCatalog::discover()
            .map(|catalog| {
                catalog
                    .list()
                    .iter()
                    .map(|skill| {
                        let mut description: String = skill.description.chars().take(32).collect();
                        if skill.description.chars().nth(32).is_some() {
                            description.push_str("...");
                        }
                        let origin = match &skill.origin {
                            crate::skills::SkillOrigin::Builtin => "built-in",
                            crate::skills::SkillOrigin::External(_) => "external",
                        };
                        crate::ui::modals::list_popup::ListPopupItem {
                            label: format!("@{} - {} [{origin}]", skill.name, description),
                            action: Some(
                                crate::ui::modals::list_popup::ListPopupAction::InsertText(
                                    format!("@{} ", skill.name),
                                ),
                            ),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::selectable(
            "Skills",
            "No skills found.",
            items,
        ));
    }

    pub(crate) fn show_skill_popup(&mut self, name: &str) {
        let content = crate::skills::SkillCatalog::discover()
            .and_then(|catalog| catalog.load(name))
            .ok()
            .flatten()
            .map(|skill| skill.source.lines().map(str::to_string).collect::<Vec<_>>())
            .unwrap_or_default();
        self.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::new(
            format!("@{name}"),
            "Skill not found.",
            content,
        ));
    }

    pub(crate) fn show_mcp_popup(&mut self) {
        let config = self
            .config
            .try_read()
            .map(|config| config.clone())
            .unwrap_or_default();
        let items = crate::mcp::merged_configs(&config.mcp_servers)
            .iter()
            .map(|server| {
                let status = if server.enabled { "on" } else { "off" };
                if let Some(url) = &server.url {
                    format!("{}  [{}]  {}", server.name, status, url)
                } else if let Some(command) = &server.command {
                    let args = server.args.clone().unwrap_or_default().join(" ");
                    format!("{}  [{}]  {} {}", server.name, status, command, args)
                } else {
                    format!("{}  [{}]", server.name, status)
                }
            })
            .collect();
        self.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::new(
            "MCP Servers",
            "No MCP servers configured.",
            items,
        ));
    }

    pub(crate) fn show_theme_popup(&mut self, filter: &str) {
        let query = filter.trim().to_ascii_lowercase();
        let items = crate::theme::theme_keys()
            .into_iter()
            .filter_map(|key| {
                let label = crate::theme::theme_label(key);
                let haystack = format!("{key} {label}").to_ascii_lowercase();
                haystack.contains(&query).then_some(
                    crate::ui::modals::list_popup::ListPopupItem::action(
                        format!("{label}  [{key}]"),
                        crate::ui::modals::list_popup::ListPopupAction::SetTheme(key.to_string()),
                    ),
                )
            })
            .collect();
        self.ui.list_popup = Some(
            crate::ui::modals::list_popup::ListPopup::anchored_selectable(
                "Themes",
                "No matching themes.",
                items,
                self.current_input_anchor(),
            ),
        );
    }

    pub(crate) fn show_local_search_popup(&mut self, query: &str) {
        let items = self
            .vault
            .as_ref()
            .and_then(|vault| vault.search(query).ok())
            .map(|paths| {
                paths
                    .into_iter()
                    .map(|path| path.display().to_string())
                    .collect()
            })
            .unwrap_or_default();
        self.ui.list_popup = Some(crate::ui::modals::list_popup::ListPopup::new(
            format!("Local Search: {}", query.trim()),
            "No local matches. Configure vault_path to enable local search.",
            items,
        ));
    }
}

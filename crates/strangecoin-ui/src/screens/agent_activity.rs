//! Agent activity screen implementation

use crate::screens::Screen;
use crate::theme::ThemeManager;
use crate::i18n::Localization;
use crate::components::{Button, Card, CardText, Table, Stat};
use eframe::egui;

/// Agent activity screen showing agent status and activities
pub struct AgentActivityScreen {
    pub agents: Vec<Agent>,
    pub selected_agent: Option<String>,
    pub activities: Vec<Activity>,
    pub filter: ActivityFilter,
    pub loading: bool,
    pub error: Option<String>,
}

/// Agent information
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub status: AgentStatus,
    pub uptime: f64,
    pub last_seen: u64,
    pub location: String,
    pub version: String,
    pub score: f64,
}

/// Agent status
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentStatus {
    Online,
    Offline,
    Error,
    Maintenance,
}

/// Activity information
#[derive(Debug, Clone)]
pub struct Activity {
    pub id: String,
    pub agent_id: String,
    pub activity_type: ActivityType,
    pub description: String,
    pub timestamp: u64,
    pub success: bool,
    pub details: Option<String>,
}

/// Activity types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActivityType {
    Validation,
    Transaction,
    BlockProduction,
    Network,
    Maintenance,
    Error,
}

/// Activity filter
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActivityFilter {
    All,
    Online,
    Offline,
    Errors,
}

impl AgentActivityScreen {
    pub fn new() -> Self {
        Self {
            agents: vec![
                Agent {
                    id: "agent_001".to_string(),
                    name: "Validator 1".to_string(),
                    status: AgentStatus::Online,
                    uptime: 99.9,
                    last_seen: 1642697400,
                    location: "New York, USA".to_string(),
                    version: "1.0.0".to_string(),
                    score: 95.0,
                },
                Agent {
                    id: "agent_002".to_string(),
                    name: "Validator 2".to_string(),
                    status: AgentStatus::Online,
                    uptime: 99.5,
                    last_seen: 1642697400,
                    location: "London, UK".to_string(),
                    version: "1.0.0".to_string(),
                    score: 92.0,
                },
                Agent {
                    id: "agent_003".to_string(),
                    name: "Validator 3".to_string(),
                    status: AgentStatus::Error,
                    uptime: 95.0,
                    last_seen: 1642697400,
                    location: "Tokyo, Japan".to_string(),
                    version: "0.9.5".to_string(),
                    score: 88.0,
                },
                Agent {
                    id: "agent_004".to_string(),
                    name: "Validator 4".to_string(),
                    status: AgentStatus::Offline,
                    uptime: 85.0,
                    last_seen: 1642697400,
                    location: "Sydney, Australia".to_string(),
                    version: "1.0.0".to_string(),
                    score: 78.0,
                },
            ],
            selected_agent: None,
            activities: vec![
                Activity {
                    id: "act_001".to_string(),
                    agent_id: "agent_001".to_string(),
                    activity_type: ActivityType::Validation,
                    description: "Validated block #12345".to_string(),
                    timestamp: 1642697400,
                    success: true,
                    details: None,
                },
                Activity {
                    id: "act_002".to_string(),
                    agent_id: "agent_002".to_string(),
                    activity_type: ActivityType::Transaction,
                    description: "Processed transaction tx_001".to_string(),
                    timestamp: 1642697400,
                    success: true,
                    details: None,
                },
                Activity {
                    id: "act_003".to_string(),
                    agent_id: "agent_003".to_string(),
                    activity_type: ActivityType::Error,
                    description: "Failed to sync with network".to_string(),
                    timestamp: 1642697400,
                    success: false,
                    details: Some("Connection timeout".to_string()),
                },
                Activity {
                    id: "act_004".to_string(),
                    agent_id: "agent_004".to_string(),
                    activity_type: ActivityType::Maintenance,
                    description: "Scheduled maintenance".to_string(),
                    timestamp: 1642697400,
                    success: true,
                    details: Some("Version upgrade".to_string()),
                },
            ],
            filter: ActivityFilter::All,
            loading: false,
            error: None,
        }
    }

    /// Render into an [`egui::Context`] (strangler launcher entry point).
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        theme_manager: &ThemeManager,
        localization: &Localization,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            Screen::show(self, ui, theme_manager, localization);
        });
    }

    pub fn agents_mut(&mut self) -> &mut Vec<Agent> {
        &mut self.agents
    }

    pub fn selected_agent_mut(&mut self) -> &mut Option<String> {
        &mut self.selected_agent
    }

    pub fn activities_mut(&mut self) -> &mut Vec<Activity> {
        &mut self.activities
    }

    pub fn filter_mut(&mut self) -> &mut ActivityFilter {
        &mut self.filter
    }

    pub fn loading_mut(&mut self) -> &mut bool {
        &mut self.loading
    }

    pub fn error_mut(&mut self) -> &mut Option<String> {
        &mut self.error
    }

    pub fn filtered_activities(&self) -> Vec<&Activity> {
        self.activities.iter().filter(|activity| {
            match self.filter {
                ActivityFilter::All => true,
                ActivityFilter::Online => {
                    if let Some(agent) = self.agents.iter().find(|a| a.id == activity.agent_id) {
                        agent.status == AgentStatus::Online
                    } else {
                        false
                    }
                }
                ActivityFilter::Offline => {
                    if let Some(agent) = self.agents.iter().find(|a| a.id == activity.agent_id) {
                        agent.status == AgentStatus::Offline
                    } else {
                        false
                    }
                }
                ActivityFilter::Errors => !activity.success,
            }
        }).collect()
    }
}

impl Screen for AgentActivityScreen {
    fn show(&mut self, ui: &mut egui::Ui, theme_manager: &ThemeManager, localization: &Localization) {
        let tokens = theme_manager.tokens();
        let colors = theme_manager.colors();

        // Header
        ui.label(
            egui::RichText::new(localization.agent_activity_title())
                .size(tokens.typography.xl.size)
                .strong()
        );

        ui.add_space(tokens.spacing.lg);

        // Agents overview
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = tokens.spacing.lg;
            
            // Total agents
            Card::new()
                .title(localization.agent_activity_total_agents())
                .child(Stat::new("total_agents", "", &self.agents.len().to_string())
                    .icon("🤖")
                    .size(crate::components::StatSize::Lg)
                )
                .show(ui, theme_manager);
            
            // Online agents
            let online_count = self.agents.iter().filter(|a| a.status == AgentStatus::Online).count();
            Card::new()
                .title(localization.agent_activity_online_agents())
                .child(Stat::new("online_agents", "", &online_count.to_string())
                    .icon("✅")
                    .size(crate::components::StatSize::Lg)
                )
                .show(ui, theme_manager);
            
            // Offline agents
            let offline_count = self.agents.iter().filter(|a| a.status == AgentStatus::Offline).count();
            Card::new()
                .title(localization.agent_activity_offline_agents())
                .child(Stat::new("offline_agents", "", &offline_count.to_string())
                    .icon("❌")
                    .size(crate::components::StatSize::Lg)
                )
                .show(ui, theme_manager);
            
            // Error agents
            let error_count = self.agents.iter().filter(|a| a.status == AgentStatus::Error).count();
            Card::new()
                .title(localization.agent_activity_error_agents())
                .child(Stat::new("error_agents", "", &error_count.to_string())
                    .icon("⚠️")
                    .size(crate::components::StatSize::Lg)
                )
                .show(ui, theme_manager);
        });

        ui.add_space(tokens.spacing.lg);

        // Activity filter
        ui.horizontal(|ui| {
            ui.label("Filter:");
            
            for filter in [ActivityFilter::All, ActivityFilter::Online, ActivityFilter::Offline, ActivityFilter::Errors] {
                if ui.selectable_label(self.filter == filter, format!("{:?}", filter)).clicked() {
                    self.filter = filter;
                }
            }
        });

        ui.add_space(tokens.spacing.lg);

        // Agents list
        Card::new()
            .title(localization.agent_activity_agents())
            .child(
                Table::new()
                    .column("Name", "Name")
                    .column("Status", "Status")
                    .column("Location", "Location")
                    .column("Uptime", "Uptime")
                    .column("Score", "Score")
                    .width(120.0)
                    .data(self.agents.iter().map(|agent| {
                        vec![
                            agent.name.clone(),
                            format!("{:?}", agent.status),
                            agent.location.clone(),
                            format!("{:.1}%", agent.uptime),
                            format!("{:.1}", agent.score),
                        ]
                    }).collect()),
            )
            .show(ui, theme_manager);

        ui.add_space(tokens.spacing.lg);

        // Activities list
        // Activities list (scroll content rendered inside the card frame)
        let mut activities_card = Card::new().title(localization.agent_activity_activities());
        let activities = self.filtered_activities();
        activities_card.show_with(ui, theme_manager, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for activity in &activities {
                    Card::new()
                        .child(CardText::new(&format!("Agent: {}", activity.agent_id)))
                        .child(CardText::new(&format!("Type: {:?}", activity.activity_type)))
                        .child(CardText::new(&format!(
                            "Description: {}",
                            activity.description
                        )))
                        .child(CardText::new(&format!(
                            "Timestamp: {}",
                            activity.timestamp
                        )))
                        .child(CardText::new(&format!("Success: {}", activity.success)).color(
                            if activity.success {
                                colors.success.as_egui()
                            } else {
                                colors.error.as_egui()
                            },
                        ))
                        .child(if let Some(ref details) = activity.details {
                            CardText::new(&format!("Details: {}", details))
                        } else {
                            CardText::new("")
                        })
                        .show(ui, theme_manager);

                    ui.add_space(tokens.spacing.base);
                }
            });
        });

        ui.add_space(tokens.spacing.lg);

        // Action buttons
        ui.horizontal(|ui| {
            Button::new("Refresh Activities")
                .onclick(|| {
                    println!("Refresh activities");
                })
                .show(ui, theme_manager);

            Button::new("Export Activities")
                .variant(crate::components::ButtonVariant::Secondary)
                .onclick(|| {
                    println!("Export activities");
                })
                .show(ui, theme_manager);

            Button::new("View Agent Details")
                .variant(crate::components::ButtonVariant::Secondary)
                .disabled(self.selected_agent.is_none())
                .onclick({
                    let agent_id = self.selected_agent.clone();
                    move || {
                        if let Some(ref agent_id) = agent_id {
                            println!("View agent details: {}", agent_id);
                        }
                    }
                })
                .show(ui, theme_manager);
        });
    }

    fn title(&self, localization: &Localization) -> String {
        localization.agent_activity_title()
    }

    fn on_back(&mut self) {
        // No back navigation for agent activity screen
    }

    fn can_navigate_back(&self) -> bool {
        false
    }
}

impl Default for AgentActivityScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_activity_screen_creation() {
        let screen = AgentActivityScreen::new();
        assert_eq!(screen.agents.len(), 4);
        assert_eq!(screen.activities.len(), 4);
        assert_eq!(screen.filter, ActivityFilter::All);
        assert!(!screen.loading);
        assert!(screen.error.is_none());
    }

    #[test]
    fn test_agent_activity_screen_agents() {
        let mut screen = AgentActivityScreen::new();
        let agents = screen.agents_mut();
        
        agents.push(Agent {
            id: "agent_005".to_string(),
            name: "Validator 5".to_string(),
            status: AgentStatus::Online,
            uptime: 99.0,
            last_seen: 1642697400,
            location: "Berlin, Germany".to_string(),
            version: "1.0.0".to_string(),
            score: 90.0,
        });
        
        assert_eq!(agents.len(), 5);
    }

    #[test]
    fn test_agent_activity_screen_activities() {
        let mut screen = AgentActivityScreen::new();
        let activities = screen.activities_mut();
        
        activities.push(Activity {
            id: "act_005".to_string(),
            agent_id: "agent_001".to_string(),
            activity_type: ActivityType::BlockProduction,
            description: "Produced block #12346".to_string(),
            timestamp: 1642697400,
            success: true,
            details: None,
        });
        
        assert_eq!(activities.len(), 5);
    }

    #[test]
    fn test_agent_activity_screen_filter() {
        let mut screen = AgentActivityScreen::new();
        assert_eq!(screen.filter, ActivityFilter::All);
        
        *screen.filter_mut() = ActivityFilter::Online;
        assert_eq!(screen.filter, ActivityFilter::Online);
        
        *screen.filter_mut() = ActivityFilter::Offline;
        assert_eq!(screen.filter, ActivityFilter::Offline);
        
        *screen.filter_mut() = ActivityFilter::Errors;
        assert_eq!(screen.filter, ActivityFilter::Errors);
    }

    #[test]
    fn test_agent_activity_screen_loading() {
        let mut screen = AgentActivityScreen::new();
        assert!(!*screen.loading_mut());
        
        *screen.loading_mut() = true;
        assert!(*screen.loading_mut());
    }

    #[test]
    fn test_agent_activity_screen_error() {
        let mut screen = AgentActivityScreen::new();
        assert!(screen.error.is_none());
        
        *screen.error_mut() = Some("Failed to load activities".to_string());
        assert_eq!(screen.error, Some("Failed to load activities".to_string()));
    }

    #[test]
    fn test_agent_activity_screen_filtered_activities() {
        let screen = AgentActivityScreen::new();
        
        let all_activities = screen.filtered_activities();
        assert_eq!(all_activities.len(), 4);
        
        // Test online filter
        let mut screen = AgentActivityScreen::new();
        *screen.filter_mut() = ActivityFilter::Online;
        let online_activities = screen.filtered_activities();
        assert!(online_activities.len() <= 4);
        
        // Test offline filter
        let mut screen = AgentActivityScreen::new();
        *screen.filter_mut() = ActivityFilter::Offline;
        let offline_activities = screen.filtered_activities();
        assert!(offline_activities.len() <= 4);
        
        // Test errors filter
        let mut screen = AgentActivityScreen::new();
        *screen.filter_mut() = ActivityFilter::Errors;
        let error_activities = screen.filtered_activities();
        assert!(error_activities.len() <= 4);
    }

    #[test]
    fn test_agent_statuses() {
        assert_eq!(AgentStatus::Online, AgentStatus::Online);
        assert_eq!(AgentStatus::Offline, AgentStatus::Offline);
        assert_eq!(AgentStatus::Error, AgentStatus::Error);
        assert_eq!(AgentStatus::Maintenance, AgentStatus::Maintenance);
    }

    #[test]
    fn test_activity_types() {
        assert_eq!(ActivityType::Validation, ActivityType::Validation);
        assert_eq!(ActivityType::Transaction, ActivityType::Transaction);
        assert_eq!(ActivityType::BlockProduction, ActivityType::BlockProduction);
        assert_eq!(ActivityType::Network, ActivityType::Network);
        assert_eq!(ActivityType::Maintenance, ActivityType::Maintenance);
        assert_eq!(ActivityType::Error, ActivityType::Error);
    }

    #[test]
    fn test_activity_filters() {
        assert_eq!(ActivityFilter::All, ActivityFilter::All);
        assert_eq!(ActivityFilter::Online, ActivityFilter::Online);
        assert_eq!(ActivityFilter::Offline, ActivityFilter::Offline);
        assert_eq!(ActivityFilter::Errors, ActivityFilter::Errors);
    }
}
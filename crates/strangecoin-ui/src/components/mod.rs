//! UI components for Strangecoin
//!
//! All components follow the design system and are theme-aware.

pub mod avatar;
pub mod badge;
pub mod breadcrumb;
pub mod button;
pub mod card;
pub mod chart;
pub mod codeblock;
pub mod drawer;
pub mod input;
pub mod modal;
pub mod stat;
pub mod table;
pub mod tabs;
pub mod toast;

// Re-export commonly used components
pub use avatar::{Avatar, AvatarSize};
pub use badge::{Badge, BadgeVariant};
pub use breadcrumb::Breadcrumb;
pub use button::{Button, ButtonVariant};
pub use card::{Card, CardBuilder, CardChild, CardDivider, CardSection, CardSpacer, CardText};
pub use chart::{Chart, ChartDataPoint, ChartType};
pub use codeblock::{CodeBlock, CodeTheme};
pub use drawer::Drawer;
pub use input::{Input, InputType};
pub use modal::Modal;
pub use stat::{Stat, StatSize, StatTrend};
pub use table::{Align, Column, SortDirection, Table};
pub use tabs::{Tab, Tabs, TextTabContent};
pub use toast::{Toast, ToastLevel};

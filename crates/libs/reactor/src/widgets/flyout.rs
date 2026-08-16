use super::*;

/// Flyout attached to a button.
///
/// Two content modes:
/// - text-only: `text` rendered as a plain `TextBlock` (original behavior);
/// - rich: `content` mounts a full reactor element subtree into the flyout
///   (preferred; falls back to `text` when absent).
///
/// `open` drives the flyout via `IFlyoutBase.ShowAt` / `Hide`; `on_closed`
/// fires on light dismiss or programmatic hide so the application can keep
/// its own open-state (e.g. a header flag) in sync.
#[derive(Clone, Debug, PartialEq)]
pub struct FlyoutDef {
    pub text: String,
    pub placement: FlyoutPlacementMode,
    /// Rich element content (takes precedence over `text`). Boxed because
    /// `Button` is a direct `Element` variant - an unboxed `Element` here
    /// would make the `Element` enum recursive without indirection.
    pub content: Option<Box<Element>>,
    /// Programmatic open/close request (ShowAt/Hide).
    pub open: bool,
    /// Fired when the flyout closes (light dismiss, Esc, or `Hide`).
    pub on_closed: Option<Callback<()>>,
}

impl Default for FlyoutDef {
    fn default() -> Self {
        Self {
            text: String::new(),
            placement: FlyoutPlacementMode::default(),
            content: None,
            open: false,
            on_closed: None,
        }
    }
}

impl FlyoutDef {
    pub(crate) fn text(text: String, placement: FlyoutPlacementMode) -> Self {
        Self {
            text,
            placement,
            ..Default::default()
        }
    }

    pub(crate) fn element(content: Element, placement: FlyoutPlacementMode) -> Self {
        Self {
            text: String::new(),
            placement,
            content: Some(Box::new(content)),
            ..Default::default()
        }
    }
}

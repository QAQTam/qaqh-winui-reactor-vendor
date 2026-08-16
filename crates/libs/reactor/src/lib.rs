#![doc = include_str!("../readme.md")]
#![allow(missing_docs)]

#[allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    clippy::upper_case_acronyms,
    clippy::missing_transmute_annotations
)]
mod bindings;

mod app;
mod app_shim;
mod backend;
mod bootstrap;
#[cfg(feature = "canvas")]
mod canvas_bridge;
mod diagnostics;
mod drag;
mod element;
mod engine;
mod fault;
mod generated;
mod hooks;
mod host;
mod interaction;
mod reconciler;
mod reference;
mod style;
mod widget;
mod widgets;

pub use app::*;
pub use backend::*;
pub use bindings::AutomationHeadingLevel;
pub use bindings::AutomationLiveSetting;
pub use bindings::Color;
pub use bindings::CommandBarDefaultLabelPosition;
pub use bindings::DispatcherQueuePriority;
pub use bindings::FlyoutPlacementMode;
pub use bindings::FocusState;
pub use bindings::HorizontalAlignment;
pub use bindings::InfoBarSeverity;
pub use bindings::NavigationViewDisplayMode;
pub use bindings::NavigationViewPaneDisplayMode;
pub use bindings::Orientation;
pub use bindings::PasswordRevealMode;
pub use bindings::ScrollBarVisibility;
pub use bindings::ScrollingScrollBarVisibility;
pub use bindings::Stretch;
pub use bindings::Symbol;
pub use bindings::TeachingTipPlacementMode;
pub use bindings::TextAlignment;
pub use bindings::TextTrimming;
pub use bindings::TextWrapping;
pub use bindings::Thickness;
pub use bindings::TreeViewSelectionMode;
pub use bindings::VerticalAlignment;
pub use bindings::VirtualKey;
pub use bindings::VirtualKeyModifiers;
pub use bootstrap::*;
#[cfg(feature = "canvas")]
pub use canvas_bridge::{
    CanvasImageSource, CanvasSwapChain, DrawContext, Invalidator, animated_canvas,
    animated_canvas_with_device, canvas, canvas_invalidated,
};
pub use drag::*;
pub use element::*;
pub use engine::*;
pub use hooks::*;
pub use host::*;
pub use interaction::*;
pub use reconciler::*;
pub use reference::*;
pub use style::*;
pub use widget::*;
pub use widgets::*;
pub use windows_core::{Error, EventRevoker, Interface, Result};
pub use windows_time::{DateTime, TimeSpan};

/// 订阅合成器帧回调（vsync 对齐）。
///
/// 回调在 UI 线程的 XAML 渲染阶段执行，频率 = 显示器刷新率
/// （60Hz → 60 次/s，120Hz → 120 次/s），比 DispatcherTimer 精确
/// （16ms 请求会被 Windows 系统时钟 15.6ms 粒度合并到 ~31ms，
/// 帧率上限 33；8ms 请求只能到 15.6ms ≈ 64fps，仍跟不上 120Hz 屏）。
/// 120Hz 客户屏的流畅渲染依赖此回调驱动事件泵。
///
/// 回调是 `Fn`（不可变借用），内部可变用 `RefCell`/`use_ref`。
/// 返回的 `EventRevoker` 必须被持有（drop 即退订）；窗口不可见时
/// 合成器暂停回调，恢复后一次性补处理积压事件。
pub fn on_frame(callback: impl Fn() + 'static) -> Result<EventRevoker> {
    bindings::CompositionTarget::Rendering(move |_sender, _args| callback())
}

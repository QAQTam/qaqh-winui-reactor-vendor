use super::*;

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollViewer {
    pub key: Option<String>,
    pub modifiers: Modifiers,
    pub child: Box<Element>,
    pub horizontal_scroll_bar_visibility: ScrollBarVisibility,
    pub vertical_scroll_bar_visibility: ScrollBarVisibility,
    /// 原生缩放（ZoomMode=Enabled 时 Ctrl+滚轮 / 触摸捏合缩放）。
    /// 值为 `ScrollViewerZoomMode`（0=Disabled, 1=Enabled）。
    pub zoom_mode: Option<i32>,
    pub min_zoom_factor: Option<f64>,
    pub max_zoom_factor: Option<f64>,
    /// 滚动到底部请求（generation：应用层递增触发 reconcile diff）。
    /// backend 在 set_prop 时调用 `ChangeView(ScrollableHeight)`；
    /// 与 list_view 的 Tail 语义一致（reconcile 先于 layout，最终位置
    /// 由下一次 delta 或 sealed 后的补充请求修正）。
    pub scroll_to_bottom: Option<i32>,
}
impl Default for ScrollViewer {
    fn default() -> Self {
        Self {
            key: None,
            modifiers: Modifiers::default(),
            child: Box::new(Element::Empty),
            horizontal_scroll_bar_visibility: ScrollBarVisibility::Disabled,
            vertical_scroll_bar_visibility: ScrollBarVisibility::Auto,
            zoom_mode: None,
            min_zoom_factor: None,
            max_zoom_factor: None,
            scroll_to_bottom: None,
        }
    }
}
impl ScrollViewer {
    pub fn new(child: impl Into<Element>) -> Self {
        Self {
            child: Box::new(child.into()),
            ..Default::default()
        }
    }
}

impl Widget for ScrollViewer {
    widget_header!(ControlKind::ScrollViewer);
    fn bindings(&self) -> PropBindings {
        generated::scroll_viewer_bindings(self)
    }
    fn children(&self) -> Children<'_> {
        Children::PositionalSingle(&self.child)
    }
}

impl ScrollViewer {
    pub fn horizontal_scroll_bar_visibility(mut self, v: ScrollBarVisibility) -> Self {
        self.horizontal_scroll_bar_visibility = v;
        self
    }

    pub fn vertical_scroll_bar_visibility(mut self, v: ScrollBarVisibility) -> Self {
        self.vertical_scroll_bar_visibility = v;
        self
    }

    /// 启用/禁用原生缩放（`ScrollViewerZoomMode`：0=Disabled, 1=Enabled）。
    /// Enabled 时 Ctrl+滚轮 / 触摸双指捏合缩放内容。
    pub fn zoom_mode(mut self, mode: i32) -> Self {
        self.zoom_mode = Some(mode);
        self
    }

    /// 请求滚动到底部（generation 递增触发 reconcile diff）。
    /// 典型用法：流式内容增长时 `scroll_to_bottom(gen + 1)`。
    pub fn scroll_to_bottom(mut self, generation: i32) -> Self {
        self.scroll_to_bottom = Some(generation);
        self
    }

    pub fn min_zoom_factor(mut self, v: f64) -> Self {
        self.min_zoom_factor = Some(v);
        self
    }

    pub fn max_zoom_factor(mut self, v: f64) -> Self {
        self.max_zoom_factor = Some(v);
        self
    }
}

pub fn scroll_viewer(child: impl Into<Element>) -> ScrollViewer {
    ScrollViewer::new(child)
}

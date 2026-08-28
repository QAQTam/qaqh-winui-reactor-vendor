use super::*;

#[derive(Clone, Debug, PartialEq)]
pub struct NumberBox {
    pub key: Option<String>,
    pub modifiers: Modifiers,
    pub value: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub on_value_changed: Option<Callback<f64>>,
    pub header: Option<String>,
    pub is_enabled: bool,
    /// 旋钮步进（QAQ：压缩阈值 0.05）。
    pub small_change: Option<f64>,
    pub large_change: Option<f64>,
    /// 0=Hidden 1=Compact 2=Inline（NumberBoxSpinButtonPlacementMode）。
    pub spin_button_placement_mode: Option<i32>,
}
impl Default for NumberBox {
    fn default() -> Self {
        Self {
            key: None,
            modifiers: Modifiers::default(),
            value: 0.0,
            minimum: f64::MIN,
            maximum: f64::MAX,
            on_value_changed: None,
            header: None,
            is_enabled: true,
            small_change: None,
            large_change: None,
            spin_button_placement_mode: None,
        }
    }
}
impl NumberBox {
    pub fn new(value: f64) -> Self {
        Self {
            value,
            ..Default::default()
        }
    }
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.minimum = min;
        self.maximum = max;
        self
    }
    pub fn on_value_changed(mut self, f: impl IntoCallback<f64>) -> Self {
        self.on_value_changed = Some(f.into_callback());
        self
    }
    pub fn header(mut self, s: impl Into<String>) -> Self {
        self.header = Some(s.into());
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.is_enabled = enabled;
        self
    }
    /// 旋钮小步长（WinUI SmallChange）。
    pub fn small_change(mut self, v: f64) -> Self {
        self.small_change = Some(v);
        self
    }
    /// 旋钮大步长（WinUI LargeChange，PageUp/PageDown）。
    pub fn large_change(mut self, v: f64) -> Self {
        self.large_change = Some(v);
        self
    }
    /// 旋钮布局：0=Hidden 1=Compact 2=Inline。
    pub fn spin_button_placement_mode(mut self, v: i32) -> Self {
        self.spin_button_placement_mode = Some(v);
        self
    }
}

impl Widget for NumberBox {
    widget_header!(ControlKind::NumberBox);
    fn bindings(&self) -> PropBindings {
        let mut out = generated::number_box_bindings(self);
        if let Some(v) = self.small_change {
            out.push(Binding::Prop(Prop::SmallChange, PropValue::F64(v)));
        }
        if let Some(v) = self.large_change {
            out.push(Binding::Prop(Prop::LargeChange, PropValue::F64(v)));
        }
        if let Some(v) = self.spin_button_placement_mode {
            out.push(Binding::Prop(
                Prop::SpinButtonPlacementMode,
                PropValue::I32(v),
            ));
        }
        out
    }
}

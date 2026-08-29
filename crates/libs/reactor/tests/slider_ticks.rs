//! QAQ B1：Slider 刻度三件套绑定回归（纯 Rust，不依赖 XAML 挂载）。
//!
//! 保证 `tick_frequency/tick_placement/snaps_to` 三个 builder 的值以
//! `Prop::Tick*`/`Prop::SnapsTo` 进入绑定流——mount 层臂依赖这三个 Prop
//! 才会调用原生 `SetTick*`/`SetSnapsTo`（composer 强度柄，见
//! qaqh-winui-app docs/nextdev/composer-streamline.md 批次 B1）。

use windows_reactor::{Binding, Prop, PropValue, Slider};

#[test]
fn tick_builders_emit_corresponding_props() {
    let slider = Slider::new(1.0)
        .range(0.0, 3.0)
        .step(1.0)
        .tick_frequency(1.0)
        .tick_placement(TickPlacement::Outside)
        .snaps_to(SnapsTo::Ticks);

    let bindings = Widget::bindings(&slider);
    assert!(bindings.iter().any(|b| matches!(
        b,
        Binding::Prop(Prop::TickFrequency, PropValue::F64(v)) if (*v - 1.0).abs() < f64::EPSILON
    )));
    assert!(bindings.iter().any(|b| matches!(
        b,
        Binding::Prop(Prop::TickPlacement, PropValue::I32(v)) if *v == TickPlacement::Outside.0
    )));
    assert!(bindings.iter().any(|b| matches!(
        b,
        Binding::Prop(Prop::SnapsTo, PropValue::I32(v)) if *v == SnapsTo::Ticks.0
    )));
}

#[test]
fn default_slider_omits_tick_props() {
    let bindings = Widget::bindings(&Slider::new(0.0));
    assert!(!bindings
        .iter()
        .any(|b| matches!(b, Binding::Prop(Prop::TickFrequency, _))));
    assert!(!bindings
        .iter()
        .any(|b| matches!(b, Binding::Prop(Prop::TickPlacement, _))));
    assert!(!bindings
        .iter()
        .any(|b| matches!(b, Binding::Prop(Prop::SnapsTo, _))));
}

use windows_reactor::{SnapsTo, TickPlacement, Widget};

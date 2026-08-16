use std::rc::Rc;

use test_reactor::{Op, RecordingBackend};
use windows_reactor::{
    ControlKind, Element, Prop, PropValue, Reconciler, TabItem, TabView, TemplatedScrollRequest,
    TextAlignment, TextBlock, list_view, text_block,
};

fn reconcile(
    reconciler: &mut Reconciler<RecordingBackend>,
    old: Option<&Element>,
    new: &Element,
    id: Option<windows_reactor::ControlId>,
) -> windows_reactor::ControlId {
    reconciler
        .reconcile(old, new, id, Rc::new(|| {}))
        .expect("element should mount")
}

#[test]
fn text_alignment_uses_the_typed_winui_enum() {
    let element: Element = TextBlock::new("center").center_aligned().into();
    let mut reconciler = Reconciler::new(RecordingBackend::new());
    reconcile(&mut reconciler, None, &element, None);

    assert!(reconciler.backend.ops.iter().any(|op| matches!(
        op,
        Op::SetProp {
            prop: Prop::TextAlignment,
            value: PropValue::I32(value),
            ..
        } if *value == TextAlignment::Center.0
    )));
}

#[test]
fn tail_request_is_frozen_before_item_source_growth_and_applied_after_it() {
    let old = list_view(vec![1], |item, _| text_block(item.to_string()))
        .follow_tail(1)
        .build();
    let new = list_view(vec![1, 2], |item, _| text_block(item.to_string()))
        .follow_tail(2)
        .build();
    let mut reconciler = Reconciler::new(RecordingBackend::new());
    let id = reconcile(&mut reconciler, None, &old, None);
    reconciler.backend.clear_ops();

    reconcile(&mut reconciler, Some(&old), &new, Some(id));

    let prepare = reconciler
        .backend
        .ops
        .iter()
        .position(|op| {
            matches!(
                op,
                Op::PrepareTemplatedScroll {
                    request: TemplatedScrollRequest::FollowTail { generation: 2 },
                    ..
                }
            )
        })
        .expect("follow request should be prepared");
    let mutate = reconciler
        .backend
        .ops
        .iter()
        .position(|op| matches!(op, Op::SetTemplatedItemCount { count: 2, .. }))
        .expect("item source should grow");
    let apply = reconciler
        .backend
        .ops
        .iter()
        .position(|op| matches!(op, Op::ApplyPreparedTemplatedScroll { .. }))
        .expect("prepared request should be applied");
    assert!(
        prepare < mutate && mutate < apply,
        "ops: {:?}",
        reconciler.backend.ops
    );
}

#[test]
fn unchanged_scroll_generation_does_not_reissue_the_request() {
    let old = list_view(vec![1], |item, _| text_block(item.to_string()))
        .force_tail(7)
        .build();
    let new = list_view(vec![1, 2], |item, _| text_block(item.to_string()))
        .force_tail(7)
        .build();
    let mut reconciler = Reconciler::new(RecordingBackend::new());
    let id = reconcile(&mut reconciler, None, &old, None);
    reconciler.backend.clear_ops();

    reconcile(&mut reconciler, Some(&old), &new, Some(id));

    assert!(!reconciler.backend.ops.iter().any(|op| matches!(
        op,
        Op::PrepareTemplatedScroll { .. } | Op::ApplyPreparedTemplatedScroll { .. }
    )));
}

#[test]
fn anchor_request_retains_its_explicit_geometry() {
    let element = list_view(vec![1, 2], |item, _| text_block(item.to_string()))
        .preserve_anchor(9, 3, 12.5)
        .build();
    let mut reconciler = Reconciler::new(RecordingBackend::new());
    reconcile(&mut reconciler, None, &element, None);

    assert!(reconciler.backend.ops.iter().any(|op| matches!(
        op,
        Op::PrepareTemplatedScroll {
            request: TemplatedScrollRequest::PreserveAnchor {
                generation: 9,
                index: 3,
                viewport_offset: 12.5,
            },
            ..
        }
    )));
}

#[test]
fn tab_header_element_round_trips_to_the_text_fallback() {
    let plain: Element = TabView::new([TabItem::new("Inbox", text_block("body"))]).into();
    let rich: Element = TabView::new([
        TabItem::new("Inbox", text_block("body")).header_element(text_block("Inbox (3)"))
    ])
    .into();
    let mut reconciler = Reconciler::new(RecordingBackend::new());
    let parent = reconcile(&mut reconciler, None, &plain, None);
    let tab_id = reconciler.backend.children_of(parent)[0];
    reconciler.backend.clear_ops();

    reconcile(&mut reconciler, Some(&plain), &rich, Some(parent));
    assert!(reconciler.backend.ops.iter().any(|op| matches!(
        op,
        Op::SetHeaderElement {
            id,
            header_id: Some(_),
        } if *id == tab_id
    )));

    reconciler.backend.clear_ops();
    reconcile(&mut reconciler, Some(&rich), &plain, Some(parent));
    let clear = reconciler.backend.ops.iter().position(|op| {
        matches!(
            op,
            Op::SetHeaderElement {
                id,
                header_id: None,
            } if *id == tab_id
        )
    });
    let fallback = reconciler.backend.ops.iter().position(|op| {
        matches!(
            op,
            Op::SetProp {
                id,
                prop: Prop::Header,
                value: PropValue::Str(value),
            } if *id == tab_id && value == "Inbox"
        )
    });
    assert!(clear.is_some() && fallback.is_some() && clear < fallback);
    assert!(!reconciler.backend.ops.iter().any(|op| matches!(
        op,
        Op::Create {
            kind: ControlKind::TabViewItem,
            ..
        }
    )));
}

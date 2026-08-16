use windows_reactor::{Color, RichTextBlock, RichTextInline, RichTextParagraph, RichTextRun};

#[test]
fn run_plain_creates_default_styled_run() {
    let r = RichTextRun::plain("hello");
    assert_eq!(r.text, "hello");
    assert!(!r.is_bold);
    assert!(!r.is_italic);
    assert!(r.foreground.is_none());
    assert!(r.font_family.is_none());
}

#[test]
fn run_can_carry_a_token_foreground() {
    let mut r = RichTextRun::plain("fn");
    r.foreground = Some(Color::rgb(197, 134, 192));
    assert_eq!(r.foreground, Some(Color::rgb(197, 134, 192)));
}

#[test]
fn paragraph_holds_inlines_in_order() {
    let p = RichTextParagraph::new(vec![
        RichTextInline::Run(RichTextRun::plain("a")),
        RichTextInline::LineBreak,
        RichTextInline::Run(RichTextRun::plain("b")),
    ]);
    assert_eq!(p.inlines.len(), 3);
    assert!(matches!(p.inlines[1], RichTextInline::LineBreak));
}

#[test]
fn rich_text_builder_chain() {
    let rt = RichTextBlock::single_paragraph(vec![RichTextInline::Run(RichTextRun::plain("x"))])
        .font_size(20.0)
        .selectable()
        .wrap();
    assert_eq!(rt.font_size, Some(20.0));
    assert!(rt.is_text_selection_enabled);
    assert_eq!(rt.text_wrapping, windows_reactor::TextWrapping::Wrap);
    assert_eq!(rt.paragraphs.len(), 1);
}

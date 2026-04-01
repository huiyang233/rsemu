#[derive(Clone, Copy, Debug)]
pub struct TagBlock<'a> {
    pub content: &'a str,
    pub attrs: &'a str,
}

pub fn first_tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    first_tag_block(xml, tag)
}

pub fn first_tag_block<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    collect_tag_blocks_with_attrs(xml, tag)
        .into_iter()
        .next()
        .map(|block| block.content)
}

pub fn collect_tag_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    collect_tag_blocks_with_attrs(xml, tag)
        .into_iter()
        .map(|block| block.content)
        .collect()
}

pub fn collect_tag_blocks_with_attrs<'a>(xml: &'a str, tag: &str) -> Vec<TagBlock<'a>> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut remaining = xml;
    let mut items: Vec<TagBlock<'a>> = Vec::new();

    while let Some(start_idx) = remaining.find(&open) {
        let open_start = start_idx + open.len();
        let after_open = &remaining[open_start..];
        let Some(open_end_rel) = after_open.find('>') else {
            break;
        };
        let attrs = after_open[..open_end_rel].trim();
        let content_start = open_start + open_end_rel + 1;
        let after_tag = &remaining[content_start..];
        let Some(close_rel) = after_tag.find(&close) else {
            break;
        };
        let content_end = content_start + close_rel;
        items.push(TagBlock {
            content: remaining[content_start..content_end].trim(),
            attrs,
        });
        remaining = &remaining[content_end + close.len()..];
    }

    items
}

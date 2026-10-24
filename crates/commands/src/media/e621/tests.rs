use super::format::{parse_post_id, split_page};
use super::*;

#[test]
fn parses_trailing_page_without_losing_query() {
    assert_eq!(split_page("cat 2"), ("cat".to_owned(), 2));
    assert_eq!(split_page("cat"), ("cat".to_owned(), 1));
    assert_eq!(split_page("123"), ("123".to_owned(), 1));
    assert_eq!(split_page("cat 0"), ("cat 0".to_owned(), 1));
}

#[test]
fn extracts_ids_from_e621_links_only() {
    assert_eq!(
        parse_post_id("https://e621.net/posts/123").unwrap().get(),
        123
    );
    assert!(parse_post_id("https://e621.net.evil.example/posts/123").is_err());
    assert!(parse_post_id("https://e621.net/artists/123").is_err());
}

#[test]
fn unknown_ratings_are_not_allowed() {
    let post = E621Post {
        id: 1,
        rating: E621Rating::Unknown,
        file: lukosbot_integrations::E621File {
            width: None,
            height: None,
            extension: None,
            size: None,
            md5: None,
            url: None,
        },
        preview_url: None,
        sample_url: None,
        score: None,
        favorites: None,
        comments: None,
        description: None,
        artist_tags: Vec::new(),
        created_at: None,
        updated_at: None,
        uploader_name: None,
        sources: Vec::new(),
    };
    assert!(!post.is_allowed(&[
        E621Rating::Safe,
        E621Rating::Questionable,
        E621Rating::Explicit
    ]));
}

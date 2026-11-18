//! The Entry mapping (spec "Implementation Decisions", ADR 0015): the pure function that turns
//! an Entry into the item a Gateway lists, or leaves it out. One test per rule, each checked
//! against the JSON agentes-chat's client reads.

mod support;

use keefast_agent::gateway::gateway_item;
use keefast_core::{CustomField, EntryFields};
use secrecy::SecretString;
use serde_json::{Value, json};
use support::{FakeClock, KEEPASSXC_KDBX4, VaultFile};

/// When the fake clock stands, as `updatedAt` writes it.
const NOW: &str = "2030-03-17T17:46:40Z";

/// The item an Entry with `fields` maps to, as JSON, with its ref.
fn item(fields: EntryFields) -> (Option<Value>, String) {
    let file = VaultFile::from_fixture(KEEPASSXC_KDBX4);
    let (mut workspace, vault) = file.open(&FakeClock::at_unix_seconds(1_900_000_000));
    let root = workspace.readable_vault(vault).unwrap().vault().root().id();
    let id = workspace.create_entry(vault, root, &fields).unwrap();
    let open = workspace.readable_vault(vault).unwrap();
    let entry = open.vault().entry(id).unwrap();
    let uuid = id.to_hex();
    let hyphenated = format!(
        "{}-{}-{}-{}-{}",
        &uuid[..8],
        &uuid[8..12],
        &uuid[12..16],
        &uuid[16..20],
        &uuid[20..]
    );
    (
        gateway_item(entry).map(|item| serde_json::to_value(item).unwrap()),
        hyphenated,
    )
}

fn field(name: &str, value: &str) -> CustomField {
    CustomField {
        name: name.to_owned(),
        value: SecretString::from(value.to_owned()),
        protected: false,
    }
}

fn login(title: &str, url: &str) -> EntryFields {
    EntryFields {
        title: title.to_owned(),
        username: "arthur".to_owned(),
        password: SecretString::from("hunter2-password".to_owned()),
        url: url.to_owned(),
        ..EntryFields::default()
    }
}

#[test]
fn an_entry_with_an_https_url_and_a_password_is_a_login() {
    let (item, uuid) = item(login("GitHub", "https://github.com/login"));

    assert_eq!(
        item,
        Some(json!({
            "ref": uuid,
            "kind": "login",
            "title": "GitHub",
            "updatedAt": NOW,
            "username": "arthur",
            "websites": [{ "url": "https://github.com/login", "match": "exact" }],
        }))
    );
}

#[test]
fn a_login_takes_its_https_kp2a_urls_as_further_websites_and_leaves_out_http_ones() {
    let mut fields = login("GitHub", "https://github.com");
    fields.custom_fields = vec![
        field("KP2A_URL", "https://gist.github.com"),
        field("KP2A_URL_1", "http://insecure.github.com"),
        field("KP2A_URL_2", "https://api.github.com"),
    ];

    let (item, _) = item(fields);

    assert_eq!(
        item.unwrap()["websites"],
        json!([
            { "url": "https://github.com", "match": "exact" },
            { "url": "https://gist.github.com", "match": "exact" },
            { "url": "https://api.github.com", "match": "exact" },
        ])
    );
}

#[test]
fn an_entry_with_an_http_url_is_no_login() {
    let (item, _) = item(login("Router", "http://192.168.1.1"));

    assert_eq!(item, None);
}

#[test]
fn an_entry_with_an_https_url_but_no_password_is_no_login() {
    let mut fields = login("GitHub", "https://github.com");
    fields.password = SecretString::from(String::new());

    let (item, _) = item(fields);

    assert_eq!(item, None);
}

#[test]
fn an_entry_tagged_login_with_a_password_is_a_login_with_only_its_https_websites() {
    // What a Gateway's create makes without a website: an UPPER_SNAKE title with no URL would
    // otherwise be a key.
    let mut no_site = login("SHOP_ADMIN", "");
    no_site.tags = vec!["Login".to_owned()];
    let mut http_site = login("Router", "http://192.168.1.1");
    http_site.tags = vec!["login".to_owned()];
    let mut no_password = login("Empty", "");
    no_password.tags = vec!["login".to_owned()];
    no_password.password = SecretString::from(String::new());

    let (no_site, _) = item(no_site);
    let (http_site, _) = item(http_site);
    let (no_password, _) = item(no_password);

    let no_site = no_site.unwrap();
    assert_eq!(no_site["kind"], "login");
    assert_eq!(no_site["websites"], json!([]));
    let http_site = http_site.unwrap();
    assert_eq!(http_site["kind"], "login");
    assert_eq!(http_site["websites"], json!([]));
    assert_eq!(no_password, None);
}

#[test]
fn an_entry_tagged_key_is_a_key_named_by_its_title() {
    // With an `http` URL, so only the tag makes it a key, and it is no login.
    let fields = EntryFields {
        title: "OPENAI_API_KEY".to_owned(),
        password: SecretString::from("sk-the-value".to_owned()),
        url: "http://platform.openai.example".to_owned(),
        tags: vec!["Key".to_owned()],
        ..EntryFields::default()
    };

    let (item, uuid) = item(fields);

    assert_eq!(
        item,
        Some(json!({
            "ref": uuid,
            "kind": "key",
            "title": "OPENAI_API_KEY",
            "updatedAt": NOW,
            "keyName": "OPENAI_API_KEY",
        }))
    );
}

#[test]
fn an_entry_with_no_url_and_an_upper_snake_title_is_a_key() {
    let fields = EntryFields {
        title: "STRIPE_SECRET_KEY_2".to_owned(),
        password: SecretString::from("sk-the-value".to_owned()),
        ..EntryFields::default()
    };

    let (item, _) = item(fields);

    let item = item.unwrap();
    assert_eq!(item["kind"], "key");
    assert_eq!(item["keyName"], "STRIPE_SECRET_KEY_2");
}

#[test]
fn a_key_without_a_password_is_left_out() {
    // agentes-chat's client reads an empty value as no value, so a key needs one to be listed.
    let tagged = EntryFields {
        title: "OPENAI_API_KEY".to_owned(),
        tags: vec!["key".to_owned()],
        ..EntryFields::default()
    };
    let untagged = EntryFields {
        title: "STRIPE_SECRET_KEY".to_owned(),
        ..EntryFields::default()
    };

    let (tagged, _) = item(tagged);
    let (untagged, _) = item(untagged);

    assert_eq!(tagged, None);
    assert_eq!(untagged, None);
}

#[test]
fn a_login_with_a_blank_title_is_left_out() {
    // agentes-chat's client drops an item whose title is blank.
    for title in ["", "   "] {
        let (item, _) = item(login(title, "https://github.com"));

        assert_eq!(item, None, "{title:?}");
    }
}

#[test]
fn an_upper_snake_title_with_a_url_and_no_key_tag_is_no_key() {
    let fields = EntryFields {
        title: "STRIPE_SECRET_KEY".to_owned(),
        password: SecretString::from("sk-the-value".to_owned()),
        url: "http://dashboard.stripe.example".to_owned(),
        ..EntryFields::default()
    };

    let (item, _) = item(fields);

    assert_eq!(item, None);
}

#[test]
fn a_key_tagged_entry_whose_title_is_not_upper_snake_is_left_out() {
    for title in ["OpenAI key", "1PASSWORD", "_TOKEN", ""] {
        let fields = EntryFields {
            title: title.to_owned(),
            password: SecretString::from("sk-the-value".to_owned()),
            tags: vec!["key".to_owned()],
            ..EntryFields::default()
        };

        let (item, _) = item(fields);

        assert_eq!(item, None, "{title:?}");
    }
}

#[test]
fn an_entry_that_is_both_a_login_and_a_key_is_a_login() {
    let mut fields = login("GITHUB_TOKEN", "https://github.com");
    fields.tags = vec!["key".to_owned()];

    let (item, _) = item(fields);

    assert_eq!(item.unwrap()["kind"], "login");
}

#[test]
fn an_entry_that_is_neither_is_left_out() {
    let fields = EntryFields {
        title: "Wi-Fi at home".to_owned(),
        password: SecretString::from("the-password".to_owned()),
        ..EntryFields::default()
    };

    let (item, _) = item(fields);

    assert_eq!(item, None);
}

#[test]
fn notes_custom_fields_and_values_never_appear_in_an_item() {
    let mut login_fields = login("GitHub", "https://github.com");
    login_fields.notes = "recovery codes: 1234-5678".to_owned();
    login_fields.custom_fields = vec![field("Recovery", "recovery-field-value")];
    let mut key_fields = EntryFields {
        title: "API_TOKEN".to_owned(),
        password: SecretString::from("the-key-value".to_owned()),
        notes: "rotate monthly".to_owned(),
        ..EntryFields::default()
    };
    key_fields.custom_fields = vec![field("Scope", "scope-field-value")];

    for (fields, kept) in [
        (
            login_fields,
            &["ref", "kind", "title", "updatedAt", "username", "websites"][..],
        ),
        (
            key_fields,
            &["ref", "kind", "title", "updatedAt", "keyName"][..],
        ),
    ] {
        let (item, _) = item(fields);
        let item = item.unwrap();

        let keys: Vec<&str> = item
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut expected = kept.to_vec();
        expected.sort();
        let mut keys = keys;
        keys.sort();
        assert_eq!(keys, expected);
        let text = item.to_string();
        for hidden in [
            "hunter2",
            "the-key-value",
            "recovery",
            "rotate",
            "field-value",
        ] {
            assert!(!text.contains(hidden), "{text}");
        }
    }
}

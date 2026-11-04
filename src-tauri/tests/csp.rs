//! The window's Content-Security-Policy, read from the shipped config.
//!
//! The PDF page view draws a page as a `data:image/png;base64,…` image: the
//! worker's PNG reaches the window as a string and is never written to disk.
//! Under `default-src 'self'` alone the image is refused and the pane shows
//! its alt text over an empty box — which no jsdom test and no mock bench can
//! see, because neither applies the policy. So the policy is asked here.
//!
//! The widening is for images only. Fonts still fall back to `default-src`,
//! which is what keeps Vite from inlining them (`pr10c-fonts.sh`, "the inline
//! limit"), and nothing may run that the bundle did not ship.

use std::path::Path;

fn policy() -> Vec<(String, Vec<String>)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("tauri.conf.json reads"))
            .expect("tauri.conf.json is JSON");
    let csp = config["app"]["security"]["csp"]
        .as_str()
        .expect("the CSP is one string");
    csp.split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            let mut words = d.split_whitespace().map(str::to_string);
            let name = words.next().expect("a directive has a name");
            (name, words.collect())
        })
        .collect()
}

fn sources(name: &str) -> Vec<String> {
    policy()
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, s)| s)
        .unwrap_or_else(|| panic!("no {name} in the CSP: {:?}", policy()))
}

#[test]
fn a_page_drawn_as_a_data_image_is_allowed_and_nothing_else_is_widened() {
    assert_eq!(sources("default-src"), ["'self'"]);
    assert_eq!(sources("img-src"), ["'self'", "data:"]);
    let names: Vec<String> = policy().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["default-src", "img-src"], "a new directive is a new decision");
}

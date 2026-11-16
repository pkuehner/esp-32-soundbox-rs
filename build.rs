fn main() {
    println!("cargo:rerun-if-changed=.wifi-password");
    let password = std::fs::read_to_string(".wifi-password")
        .expect("Create .wifi-password containing the Wi-Fi access point password");
    let password = password.trim_end_matches(['\r', '\n']);
    assert!(
        (8..=63).contains(&password.len()) && password.bytes().all(|b| (32..=126).contains(&b)),
        "Wi-Fi password must contain 8-63 printable ASCII characters"
    );
    println!("cargo:rustc-env=SOUNDBOX_WIFI_PASSWORD={password}");
    embuild::espidf::sysenv::output();
}

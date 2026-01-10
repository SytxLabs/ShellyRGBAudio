fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/ShellyRGBAudio.ico");
    res.set("FileDescription", "ShellyRGBAudio");
    res.set("ProductName", "ShellyRGBAudio");
    res.set("CompanyName", "SytxLabs");
    res.set("LegalCopyright", "© 2026 SytxLabs");
    res.set("OriginalFilename", "ShellyRGBAudio.exe");
    res.compile().unwrap();
}
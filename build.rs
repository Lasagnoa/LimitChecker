fn main() {
    // Embed ico.ico as the exe icon via Windows resource
    let mut res = winresource::WindowsResource::new();
    res.set_icon("icon.ico");
    res.compile().expect("Failed to compile Windows resource");
}

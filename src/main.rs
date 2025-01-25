use win32_display_data::connected_displays_physical;

fn main() {
    for (i, _) in connected_displays_physical().enumerate() {
        println!("^^^^^^^^^^^^^^^^^^^^^^ this is the display device at index {i} =========================");
    }
}
fn main() {
    qtrs_build::Ui::embed().expect("embed ui files");
    qtrs_build::Rcc::embed().expect("embed qrc files");
}

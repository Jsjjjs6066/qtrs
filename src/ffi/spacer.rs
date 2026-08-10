unsafe extern "C++" {
        include!("src/cpp/qt_widget.h");

        unsafe fn QSpacerItem_new(w: i32, h: i32, hPolicy: i32, vPolicy: i32) -> *mut QSpacerItem;
        unsafe fn QSpacerItem_delete(s: *mut QSpacerItem);
        unsafe fn QSpacerItem_changeSize(s: *mut QSpacerItem, w: i32, h: i32, hPolicy: i32, vPolicy: i32);
    }

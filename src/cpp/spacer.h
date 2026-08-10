// src/cpp/spacer.h — QSpacerItem
#pragma once

#include <QtWidgets/QSizePolicy>
#include <QtWidgets/QSpacerItem>

inline QSpacerItem *QSpacerItem_new(int w, int h, int hPolicy, int vPolicy) {
    return new QSpacerItem(w, h,
        static_cast<QSizePolicy::Policy>(hPolicy),
        static_cast<QSizePolicy::Policy>(vPolicy));
}

inline void QSpacerItem_delete(QSpacerItem *s) { delete s; }

inline void QSpacerItem_changeSize(QSpacerItem *s, int w, int h, int hPolicy, int vPolicy) {
    s->changeSize(w, h,
        static_cast<QSizePolicy::Policy>(hPolicy),
        static_cast<QSizePolicy::Policy>(vPolicy));
}

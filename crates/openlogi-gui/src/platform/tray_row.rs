//! Custom `NSMenuItem` row views for the menu-bar device list.

#![expect(
    unsafe_code,
    reason = "AppKit NSView FFI for tray menu rows; GPUI has no menu-bar API"
)]

use std::collections::HashMap;
use std::sync::{Mutex, Once, OnceLock};

use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use openlogi_core::device::DeviceKind;

const ROW_WIDTH: f64 = 240.0;
const ROW_HEIGHT: f64 = 28.0;
const ICON_SIZE: f64 = 16.0;
const DONUT_SIZE: f64 = 22.0;
const BATTERY_LABEL_W: f64 = 36.0;

fn donut_state() -> &'static Mutex<HashMap<usize, Option<u8>>> {
    static STATE: OnceLock<Mutex<HashMap<usize, Option<u8>>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A retained row view installed on a tray `NSMenuItem`.
#[derive(Clone, Copy)]
pub(super) struct TrayRowView(usize);

impl TrayRowView {
    /// Device row with icon, primary name, battery value label, and donut.
    /// Subview order: [0]=icon  [1]=name  [2]=bat_label  [3]=donut
    pub(super) fn new_device() -> Self {
        register_donut_class();
        let view = plain_row_view();
        set_frame(view, ROW_WIDTH, ROW_HEIGHT);

        let icon = image_view(ICON_SIZE, ICON_SIZE);
        position_view(icon, 10.0, (ROW_HEIGHT - ICON_SIZE) / 2.0, ICON_SIZE, ICON_SIZE);
        add_subview(view, icon);

        let name = text_label("", 13.0, false);
        position_label(name, 32.0, 5.0, name_label_width());
        add_subview(view, name);

        let bat = text_label("", 11.0, true);
        position_label(bat, bat_label_x(), 5.0, BATTERY_LABEL_W);
        unsafe {
            let _: () = msg_send![bat, setAlignment: 2_i64]; // NSTextAlignmentCenter
        }
        add_subview(view, bat);

        let donut = donut_view();
        position_view(
            donut,
            donut_x(),
            (ROW_HEIGHT - DONUT_SIZE) / 2.0,
            DONUT_SIZE,
            DONUT_SIZE,
        );
        add_subview(view, donut);

        Self(view as usize)
    }

    /// Empty-state row: muted message, no icon, battery label, or donut.
    pub(super) fn update_empty(&self, message: &str) {
        let view = self.raw();
        unsafe {
            let subviews: id = msg_send![view, subviews];
            let count: usize = msg_send![subviews, count];
            if count >= 4 {
                let icon: id = msg_send![subviews, objectAtIndex: 0];
                let label: id = msg_send![subviews, objectAtIndex: 1];
                let bat: id = msg_send![subviews, objectAtIndex: 2];
                let donut: id = msg_send![subviews, objectAtIndex: 3];
                let _: () = msg_send![icon, setHidden: YES];
                let _: () = msg_send![bat, setHidden: YES];
                let _: () = msg_send![donut, setHidden: YES];
                let _: () = msg_send![label, setStringValue: nsstring(message)];
                let color: id = msg_send![class!(NSColor), secondaryLabelColor];
                let _: () = msg_send![label, setTextColor: color];
                position_label(label, 12.0, 5.0, ROW_WIDTH - 24.0);
            }
        }
    }

    pub(super) fn update_device(
        &self,
        name: &str,
        kind: DeviceKind,
        battery_percent: Option<u8>,
    ) {
        let view = self.raw();
        unsafe {
            let subviews: id = msg_send![view, subviews];
            let count: usize = msg_send![subviews, count];
            if count >= 4 {
                let icon: id = msg_send![subviews, objectAtIndex: 0];
                let label: id = msg_send![subviews, objectAtIndex: 1];
                let bat: id = msg_send![subviews, objectAtIndex: 2];
                let donut: id = msg_send![subviews, objectAtIndex: 3];

                if let Some(image) = symbol_image(device_symbol(kind), ICON_SIZE) {
                    let _: () = msg_send![icon, setImage: image];
                    let _: () = msg_send![icon, setHidden: NO];
                } else {
                    let _: () = msg_send![icon, setHidden: YES];
                }

                let _: () = msg_send![label, setStringValue: nsstring(name)];
                let primary: id = msg_send![class!(NSColor), labelColor];
                let _: () = msg_send![label, setTextColor: primary];
                position_label(label, 32.0, 5.0, name_label_width());

                if let Some(percent) = battery_percent {
                    let text = format!("{percent}%");
                    let _: () = msg_send![bat, setStringValue: nsstring(&text)];
                    let _: () = msg_send![bat, setHidden: NO];
                    let _: () = msg_send![donut, setHidden: NO];
                    set_donut_percent(donut, Some(percent));
                } else {
                    let _: () = msg_send![bat, setHidden: YES];
                    let _: () = msg_send![donut, setHidden: YES];
                    set_donut_percent(donut, None);
                }
            }
        }
    }

    pub(super) fn raw(&self) -> id {
        self.0 as id
    }
}

// Layout helpers — keep arithmetic in one place.
fn donut_x() -> f64 {
    ROW_WIDTH - DONUT_SIZE - 10.0
}
fn bat_label_x() -> f64 {
    donut_x() - 4.0 - BATTERY_LABEL_W
}
fn name_label_width() -> f64 {
    bat_label_x() - 32.0 - 4.0
}

fn device_symbol(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Keyboard | DeviceKind::Numpad => "keyboard",
        DeviceKind::Mouse | DeviceKind::Trackball | DeviceKind::Touchpad => "computermouse",
        DeviceKind::Headset => "headphones",
        DeviceKind::Gamepad | DeviceKind::Joystick => "gamecontroller",
        _ => "computermouse",
    }
}

fn plain_row_view() -> id {
    unsafe {
        let view: id = msg_send![class!(NSView), alloc];
        let view: id = msg_send![view, initWithFrame: row_rect(ROW_WIDTH, ROW_HEIGHT)];
        let _: id = msg_send![view, retain];
        view
    }
}

fn image_view(width: f64, height: f64) -> id {
    unsafe {
        let view: id = msg_send![class!(NSImageView), alloc];
        let view: id = msg_send![view, initWithFrame: row_rect(width, height)];
        let _: () = msg_send![view, setImageScaling: 2_i64]; // NSImageScaleProportionallyUpOrDown
        let _: () = msg_send![view, setImageAlignment: 1_i64]; // NSImageAlignCenter
        let _: id = msg_send![view, retain];
        view
    }
}

fn text_label(text: &str, size: f64, muted: bool) -> id {
    unsafe {
        let field: id = msg_send![class!(NSTextField), alloc];
        let field: id = msg_send![field, initWithFrame: row_rect(100.0, 18.0)];
        let _: () = msg_send![field, setStringValue: nsstring(text)];
        let _: () = msg_send![field, setBezeled: NO];
        let _: () = msg_send![field, setDrawsBackground: NO];
        let _: () = msg_send![field, setEditable: NO];
        let _: () = msg_send![field, setSelectable: NO];
        let font: id = msg_send![
            class!(NSFont),
            systemFontOfSize: size
        ];
        let _: () = msg_send![field, setFont: font];
        let color: id = if muted {
            msg_send![class!(NSColor), secondaryLabelColor]
        } else {
            msg_send![class!(NSColor), labelColor]
        };
        let _: () = msg_send![field, setTextColor: color];
        let _: id = msg_send![field, retain];
        field
    }
}

fn donut_view() -> id {
    register_donut_class();
    unsafe {
        let cls = Class::get("OpenLogiBatteryDonut").unwrap_or_else(|| class!(NSView));
        let view: id = msg_send![cls, alloc];
        let view: id = msg_send![view, initWithFrame: row_rect(DONUT_SIZE, DONUT_SIZE)];
        let _: id = msg_send![view, retain];
        view
    }
}

fn register_donut_class() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        if let Some(mut decl) = ClassDecl::new("OpenLogiBatteryDonut", class!(NSView)) {
            unsafe {
                decl.add_method(
                    sel!(drawRect:),
                    draw_donut as extern "C" fn(&Object, Sel, NSRect),
                );
            }
            decl.register();
        }
    });
}

extern "C" fn draw_donut(this: &Object, _sel: Sel, _rect: NSRect) {
    let ptr = std::ptr::from_ref(this) as usize;
    let percent = donut_state()
        .lock()
        .ok()
        .and_then(|map| map.get(&ptr).copied())
        .flatten();

    let Some(percent) = percent else {
        return;
    };

    unsafe {
        let bounds: NSRect = msg_send![this, bounds];
        let cx = bounds.origin.x + bounds.size.width / 2.0;
        let cy = bounds.origin.y + bounds.size.height / 2.0;
        let radius = bounds.size.width / 2.0 - 2.0;
        let line_width = 3.0;

        let track: id = msg_send![class!(NSBezierPath), bezierPath];
        let _: () = msg_send![track, appendBezierPathWithArcWithCenter: NSPoint::new(cx, cy) radius: radius startAngle: 0.0 endAngle: 360.0];
        let track_color: id = msg_send![class!(NSColor), separatorColor];
        let _: () = msg_send![track_color, set];
        let _: () = msg_send![track, setLineWidth: line_width];
        let _: () = msg_send![track, stroke];

        let fill: id = msg_send![class!(NSBezierPath), bezierPath];
        let end_angle = 90.0 - (f64::from(percent) / 100.0) * 360.0;
        let _: () = msg_send![fill, appendBezierPathWithArcWithCenter: NSPoint::new(cx, cy) radius: radius startAngle: 90.0 endAngle: end_angle clockwise: YES];
        let fill_color: id = battery_color(percent);
        let _: () = msg_send![fill_color, set];
        let _: () = msg_send![fill, setLineWidth: line_width];
        let _: () = msg_send![fill, stroke];
    }
}

fn battery_color(percent: u8) -> id {
    unsafe {
        if percent <= 20 {
            msg_send![class!(NSColor), systemRedColor]
        } else if percent <= 50 {
            msg_send![class!(NSColor), systemYellowColor]
        } else {
            msg_send![class!(NSColor), systemGreenColor]
        }
    }
}

fn set_donut_percent(view: id, percent: Option<u8>) {
    let ptr = view as usize;
    if let Ok(mut map) = donut_state().lock() {
        map.insert(ptr, percent);
    }
    unsafe {
        let _: () = msg_send![view, setNeedsDisplay: YES];
    }
}

fn symbol_image(symbol: &str, size: f64) -> Option<id> {
    unsafe {
        let image: id = msg_send![
            class!(NSImage),
            imageWithSystemSymbolName: nsstring(symbol)
            accessibilityDescription: nil
        ];
        if image == nil {
            return None;
        }
        let rect = row_rect(size, size);
        let _: () = msg_send![image, setSize: rect.size];
        let _: () = msg_send![image, setTemplate: YES];
        Some(image)
    }
}

fn row_rect(width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height))
}

fn set_frame(view: id, width: f64, height: f64) {
    unsafe {
        let _: () = msg_send![view, setFrame: row_rect(width, height)];
    }
}

fn position_view(view: id, x: f64, y: f64, width: f64, height: f64) {
    unsafe {
        let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(width, height));
        let _: () = msg_send![view, setFrame: frame];
    }
}

fn position_label(label: id, x: f64, y: f64, width: f64) {
    position_view(label, x, y, width, 18.0);
}

fn add_subview(parent: id, child: id) {
    unsafe {
        let _: () = msg_send![parent, addSubview: child];
    }
}

fn nsstring(s: &str) -> id {
    unsafe { NSString::alloc(nil).init_str(s) }
}

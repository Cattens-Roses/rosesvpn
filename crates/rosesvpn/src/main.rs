use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Hsla, Render, Rgba, ScrollHandle, SharedString,
    TitlebarOptions, Window, WindowBounds, WindowOptions, canvas, div, point, px,
};
use std::cell::Cell;
use std::f32::consts::TAU;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;
use ui::{
    ActiveTheme as _, Button, Look, MenuItem, Motion, Motioned, Pace, Picker, Popovers, Rounding,
    Scrollbar, Scroller, Scrubber, ScrubberState, Separator, Stillness, Switch, TabBar, Theme,
    ThemeKind, ThemeOverrides, WindowControls,
};

const FIRST_SIZE: gpui::Size<gpui::Pixels> = gpui::size(px(920.), px(640.));
const AMBIENT_DOWNSCALE: f32 = 3.;
const AMBIENT_BLUR: f32 = 4.;
const AMBIENT_DISCS: usize = 18;
const AMBIENT_STYLE_FADE: std::time::Duration = std::time::Duration::from_millis(320);
const AMBIENT_SPECS: [(f32, f32, f32, f32, f32, f32, f32); 5] = [
    (0.2, 0.27, 1.0, 31., 0.4, 0.12, 0.1),
    (0.78, 0.22, 0.95, 38., 2.1, 0.1, 0.12),
    (0.52, 0.7, 1.15, 29., 3.6, 0.13, 0.09),
    (0.15, 0.76, 0.84, 43., 5.2, 0.1, 0.11),
    (0.85, 0.76, 0.9, 35., 2.7, 0.11, 0.1),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Appearance,
    Security,
}

#[derive(Clone, Copy)]
enum PrivilegedOperation {
    Connect,
    Disconnect,
    SetLockdown(bool),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AmbientStyle {
    Aurora,
    Sunset,
    Ocean,
    Forest,
    Rose,
}

impl AmbientStyle {
    const ALL: [Self; 5] = [
        Self::Aurora,
        Self::Sunset,
        Self::Ocean,
        Self::Forest,
        Self::Rose,
    ];

    fn id(self) -> &'static str {
        match self {
            Self::Aurora => "aurora",
            Self::Sunset => "sunset",
            Self::Ocean => "ocean",
            Self::Forest => "forest",
            Self::Rose => "rose",
        }
    }

    fn from_id(id: &str) -> Self {
        match id {
            "sunset" => Self::Sunset,
            "ocean" => Self::Ocean,
            "forest" => Self::Forest,
            "rose" => Self::Rose,
            _ => Self::Aurora,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Aurora => "Aurora",
            Self::Sunset => "Sunset",
            Self::Ocean => "Ocean",
            Self::Forest => "Forest",
            Self::Rose => "Rose",
        }
    }

    fn colors(self) -> (Hsla, Hsla) {
        let color = |h, s, l| Hsla { h, s, l, a: 1. };
        match self {
            Self::Aurora => (color(0.48, 0.78, 0.48), color(0.76, 0.72, 0.52)),
            Self::Sunset => (color(0.02, 0.84, 0.54), color(0.90, 0.78, 0.48)),
            Self::Ocean => (color(0.58, 0.82, 0.48), color(0.48, 0.74, 0.52)),
            Self::Forest => (color(0.36, 0.72, 0.42), color(0.48, 0.66, 0.48)),
            Self::Rose => (color(0.94, 0.76, 0.56), color(0.78, 0.72, 0.56)),
        }
    }

    fn blob_colors(self) -> [Hsla; 5] {
        let (primary, secondary) = self.colors();
        let base = Hsla { l: 0.39, ..primary };
        let second = Hsla {
            l: 0.38,
            ..secondary
        };
        [
            base,
            Hsla {
                l: 0.53,
                s: (base.s - 0.08).clamp(0.4, 0.78),
                ..base
            },
            second,
            Hsla {
                l: 0.27,
                s: (base.s + 0.04).clamp(0.45, 0.82),
                ..base
            },
            Hsla {
                h: (base.h + 0.97).rem_euclid(1.),
                l: 0.25,
                ..base
            },
        ]
    }
}

#[derive(Clone, Copy)]
struct AmbientTransition {
    from: [Hsla; 5],
    started: Instant,
}

struct RosesVpn {
    page: Page,
    theme: ThemeKind,
    rounding: Rounding,
    blur: bool,
    blur_window: bool,
    font_size: f32,
    icon_pack: usize,
    sidebar_open: bool,
    transparent: bool,
    transparency: f32,
    opacity: ScrubberState,
    popovers: Popovers,
    stillness: Stillness,
    pace: Pace,
    ambient: bool,
    ambient_style: AmbientStyle,
    ambient_transition: Option<AmbientTransition>,
    ambient_motion: bool,
    ambient_started: Instant,
    ambient_bounds: Rc<Cell<Bounds<gpui::Pixels>>>,
    focus: FocusHandle,
    scrollbar: Entity<Scrollbar>,
    appearance_observer: Option<gpui::Subscription>,
    vpn_status: String,
    transfer_stats: vpn_engine::TransferStats,
    stats_poll_started: bool,
    profiles: Vec<vpn_engine::VpnProfile>,
    selected_profile: Option<usize>,
    lockdown: bool,
    lockdown_service_active: bool,
    security_notice: Option<String>,
    privilege_prompt: Option<PrivilegedOperation>,
}

#[derive(Clone, Copy)]
struct Preferences {
    theme: ThemeKind,
    rounding: Rounding,
    blur: bool,
    blur_window: bool,
    font_size: f32,
    icon_pack: &'static str,
    transparent: bool,
    transparency: f32,
    stillness: Stillness,
    pace: Pace,
    ambient: bool,
    ambient_style: AmbientStyle,
    ambient_motion: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: ThemeKind::System,
            rounding: Rounding::Subtle,
            blur: true,
            blur_window: false,
            font_size: 14.,
            icon_pack: icons::BASE,
            transparent: false,
            transparency: 0.,
            stillness: Stillness::System,
            pace: Pace::Base,
            ambient: false,
            ambient_style: AmbientStyle::Aurora,
            ambient_motion: true,
        }
    }
}

impl Preferences {
    fn load() -> Self {
        let mut prefs = Self::default();
        let Some(path) = preferences_path() else {
            return prefs;
        };
        let Ok(contents) = std::fs::read_to_string(path) else {
            return prefs;
        };
        for line in contents.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "theme" => prefs.theme = ThemeKind::from_id(value),
                "rounding" => prefs.rounding = Rounding::from_id(value),
                "blur" => prefs.blur = value.parse().unwrap_or(prefs.blur),
                "blur_window" => prefs.blur_window = value.parse().unwrap_or(prefs.blur_window),
                "font_size" => {
                    prefs.font_size = value
                        .parse::<f32>()
                        .unwrap_or(prefs.font_size)
                        .clamp(ui::MIN_FONT, ui::MAX_FONT)
                }
                "icon_pack" if icons::pack(value).is_some() => {
                    prefs.icon_pack = icons::pack(value).unwrap().id
                }
                "transparent" => prefs.transparent = value.parse().unwrap_or(prefs.transparent),
                "transparency" => {
                    prefs.transparency = value
                        .parse::<f32>()
                        .unwrap_or(prefs.transparency)
                        .clamp(0., ui::MAX_TRANSPARENCY)
                }
                "stillness" => prefs.stillness = Stillness::from_id(value),
                "pace" => prefs.pace = Pace::from_id(value),
                "ambient" => prefs.ambient = value.parse().unwrap_or(prefs.ambient),
                "ambient_style" => prefs.ambient_style = AmbientStyle::from_id(value),
                "ambient_motion" => {
                    prefs.ambient_motion = value.parse().unwrap_or(prefs.ambient_motion)
                }
                _ => {}
            }
        }
        prefs.transparent |= prefs.transparency > 0.;
        prefs
    }

    fn save(self) {
        let Some(path) = preferences_path() else {
            return;
        };
        let Some(parent) = path.parent() else { return };
        if let Err(error) = std::fs::create_dir_all(parent).and_then(|()| {
            std::fs::write(path, format!(
                "theme={}\nrounding={}\nblur={}\nblur_window={}\nfont_size={}\nicon_pack={}\ntransparent={}\ntransparency={}\nstillness={}\npace={}\nambient={}\nambient_style={}\nambient_motion={}\n",
                self.theme.id(), self.rounding.id(), self.blur, self.blur_window, self.font_size,
                self.icon_pack, self.transparent, self.transparency, self.stillness.id(), self.pace.id(),
                self.ambient, self.ambient_style.id(), self.ambient_motion
            ))
        }) {
            log::warn!("Could not save appearance preferences: {error}");
        }
    }
}

fn preferences_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)?
        .join("Library/Application Support");
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("RosesVPN").join("appearance.cfg"))
}

fn choose_wireguard_config() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::Controls::Dialogs::{
            GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW,
        };
        let mut file = [0u16; 32768];
        let filter = "WireGuard configuration (*.conf)\0*.conf\0All files (*.*)\0*.*\0\0"
            .encode_utf16()
            .collect::<Vec<_>>();
        let title = "Import WireGuard configuration\0"
            .encode_utf16()
            .collect::<Vec<_>>();
        let mut dialog = OPENFILENAMEW::default();
        dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = file.as_mut_ptr();
        dialog.nMaxFile = file.len() as u32;
        dialog.lpstrTitle = title.as_ptr();
        dialog.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;
        if GetOpenFileNameW(&mut dialog) == 0 {
            return None;
        }
        let length = file.iter().position(|c| *c == 0).unwrap_or(file.len());
        return Some(PathBuf::from(String::from_utf16_lossy(&file[..length])));
    }
    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("zenity")
            .args([
                "--file-selection",
                "--file-filter=WireGuard (*.conf) | *.conf",
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(PathBuf::from(
            String::from_utf8_lossy(&output.stdout).trim(),
        ))
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("osascript")
            .args([
                "-e",
                "POSIX path of (choose file with prompt \"Import WireGuard configuration\")",
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(PathBuf::from(
            String::from_utf8_lossy(&output.stdout).trim(),
        ))
    }
}

fn run_privileged_operation(
    operation: PrivilegedOperation,
    profile_path: Option<PathBuf>,
    connected: bool,
) -> anyhow::Result<()> {
    match operation {
        PrivilegedOperation::Connect => {
            let path = profile_path.ok_or_else(|| anyhow::anyhow!("Import a VPN profile first"))?;
            vpn_engine::connect(&path).map(|_| ())
        }
        PrivilegedOperation::Disconnect => {
            let path = profile_path.ok_or_else(|| anyhow::anyhow!("Select a VPN profile first"))?;
            vpn_engine::disconnect_config(&path)
        }
        PrivilegedOperation::SetLockdown(enabled) => {
            vpn_engine::set_lockdown(enabled, connected, profile_path.as_deref())
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

impl RosesVpn {
    fn poll_transfer_stats(&mut self, cx: &mut Context<Self>) {
        if self.stats_poll_started {
            return;
        }
        self.stats_poll_started = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let path = this
                    .update(cx, |this, _| {
                        if this.vpn_status != "Connected" {
                            None
                        } else {
                            this.selected_profile
                                .and_then(|index| this.profiles.get(index))
                                .map(|profile| profile.path.clone())
                        }
                    })
                    .ok()
                    .flatten();
                let Some(path) = path else { break };
                let stats = cx
                    .background_executor()
                    .spawn(async move { vpn_engine::transfer_stats(&path).ok() })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if let Some(stats) = stats {
                            this.transfer_stats = stats;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn run_privileged_operation(&mut self, operation: PrivilegedOperation, cx: &mut Context<Self>) {
        self.privilege_prompt = None;
        let profile_path = self
            .selected_profile
            .and_then(|index| self.profiles.get(index))
            .map(|profile| profile.path.clone());
        let connected = self.vpn_status == "Connected";
        self.vpn_status = match operation {
            PrivilegedOperation::Connect => "Connecting".into(),
            PrivilegedOperation::Disconnect => "Disconnecting".into(),
            PrivilegedOperation::SetLockdown(_) => self.vpn_status.clone(),
        };
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { run_privileged_operation(operation, profile_path, connected) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => match operation {
                        PrivilegedOperation::Connect => {
                            this.vpn_status = "Connected".into();
                            if this.vpn_status == "Connected" {
                                this.transfer_stats = vpn_engine::TransferStats::default();
                                this.poll_transfer_stats(cx);
                            }
                        }
                        PrivilegedOperation::Disconnect => {
                            this.vpn_status = "Disconnected".into();
                            this.stats_poll_started = false;
                            this.transfer_stats = vpn_engine::TransferStats::default();
                        }
                        PrivilegedOperation::SetLockdown(enabled) => {
                            this.lockdown = enabled;
                            this.security_notice = None;
                            this.save();
                        }
                    },
                    Err(error) => match operation {
                        PrivilegedOperation::SetLockdown(_) => {
                            this.security_notice =
                                Some(format!("Could not change Lockdown Mode: {error:#}"))
                        }
                        PrivilegedOperation::Connect => {
                            this.vpn_status = format!("Could not connect: {error:#}");
                        }
                        PrivilegedOperation::Disconnect => {
                            this.vpn_status = format!("Could not disconnect: {error:#}");
                        }
                    },
                }
                this.lockdown_service_active = vpn_engine::lockdown_active();
                cx.notify();
            });
        })
        .detach();
    }

    fn new(prefs: Preferences, cx: &mut Context<Self>) -> Self {
        let owner = cx.entity_id();
        let profiles = vpn_engine::profiles().unwrap_or_default();
        let selected_profile = (!profiles.is_empty()).then_some(0);
        Self {
            page: Page::Home,
            theme: prefs.theme,
            rounding: prefs.rounding,
            blur: prefs.blur,
            blur_window: prefs.blur_window,
            font_size: prefs.font_size,
            icon_pack: icons::packs()
                .position(|pack| pack.id == prefs.icon_pack)
                .unwrap_or_default(),
            sidebar_open: true,
            transparent: prefs.transparent,
            transparency: prefs.transparency,
            opacity: ScrubberState::new("appearance-opacity"),
            popovers: Popovers::default(),
            stillness: prefs.stillness,
            pace: prefs.pace,
            ambient: prefs.ambient,
            ambient_style: prefs.ambient_style,
            ambient_transition: None,
            ambient_motion: prefs.ambient_motion,
            ambient_started: Instant::now(),
            ambient_bounds: Rc::new(Cell::new(Bounds::default())),
            focus: cx.focus_handle(),
            scrollbar: cx.new(|_| Scrollbar::new(ScrollHandle::new()).watching(owner)),
            appearance_observer: None,
            vpn_status: selected_profile
                .and_then(|index| profiles.get(index))
                .map(|profile| vpn_engine::tunnel_status_for(&profile.path))
                .unwrap_or_else(|| "Disconnected".into()),
            transfer_stats: vpn_engine::TransferStats::default(),
            stats_poll_started: false,
            profiles,
            selected_profile,
            lockdown: vpn_engine::lockdown_active(),
            lockdown_service_active: vpn_engine::lockdown_active(),
            security_notice: None,
            privilege_prompt: None,
        }
    }

    fn look(&self) -> Look {
        Look {
            kind: self.theme,
            rounding: self.rounding,
            font: self.font_size,
            transparent: self.transparent,
            transparency: self.transparency,
            blur: self.blur,
            blur_window: self.blur_window,
            tint: self.ambient.then(|| self.ambient_style.colors().0),
            tint_secondary: self.ambient.then(|| self.ambient_style.colors().1),
        }
    }

    fn ambient_colors(&self, now: Instant) -> [Hsla; 5] {
        let target = self.ambient_style.blob_colors();
        let Some(transition) = self.ambient_transition else {
            return target;
        };
        let progress = (now.duration_since(transition.started).as_secs_f32()
            / AMBIENT_STYLE_FADE.as_secs_f32())
        .clamp(0., 1.);
        let progress = ui::ease_out_cubic(progress);
        std::array::from_fn(|index| ui::mix(transition.from[index], target[index], progress))
    }

    fn apply(&self, cx: &mut Context<Self>) {
        Theme::set(self.look(), &ThemeOverrides::default(), cx);
    }

    fn preferences(&self) -> Preferences {
        Preferences {
            theme: self.theme,
            rounding: self.rounding,
            blur: self.blur,
            blur_window: self.blur_window,
            font_size: self.font_size,
            icon_pack: icons::packs()
                .nth(self.icon_pack)
                .map_or(icons::BASE, |pack| pack.id),
            transparent: self.transparent,
            transparency: self.transparency,
            stillness: self.stillness,
            pace: self.pace,
            ambient: self.ambient,
            ambient_style: self.ambient_style,
            ambient_motion: self.ambient_motion,
        }
    }

    fn save(&self) {
        self.preferences().save();
    }

    fn row(
        &self,
        title: impl Into<SharedString>,
        detail: impl Into<SharedString>,
        action: gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = *cx.theme();
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .w_full()
            .py_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_color(theme.foreground).child(title.into()))
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .text_size(theme.text(ui::Text::Small))
                            .child(detail.into()),
                    ),
            )
            .child(div().flex_none().child(action))
    }
}

impl Render for RosesVpn {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let sidebar_open = self.sidebar_open;
        let sidebar = div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(208.))
            .h_full()
            .p_3()
            .when(!theme.transparent && !self.ambient, |sidebar| {
                sidebar.bg(theme.sidebar)
            })
            .border_r_1()
            .border_color(theme.sidebar_border)
            .children(
                [
                    (Page::Home, "nav-vpn", "icons/lock.svg"),
                    (Page::Security, "nav-security", "icons/lock.svg"),
                    (Page::Appearance, "nav-settings", "icons/settings.svg"),
                ]
                .into_iter()
                .map(|(page, label, icon)| {
                    let selected = self.page == page;
                    let tint = if selected {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    };
                    Button::new(format!("nav-{label}"))
                        .ghost()
                        .label(match page {
                            Page::Home => "VPN".into(),
                            Page::Security => "Security".into(),
                            Page::Appearance => i18n::lookup(label, None),
                        })
                        .icon(icons::path(icon))
                        .tint(tint)
                        .gap_2p5()
                        .justify_start()
                        .hover(move |style| style.bg(theme.sidebar_accent))
                        .active(move |style| style.bg(theme.sidebar_accent))
                        .when(selected, |button| button.bg(theme.sidebar_accent))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.page = page;
                            cx.notify();
                        }))
                }),
            );

        let content = match self.page {
            Page::Home => {
                let status = self.vpn_status.clone();
                let selected = self.selected_profile;
                let profiles = self.profiles.clone();
                let selected_label = selected.and_then(|index| profiles.get(index))
                    .map(|profile| format!("{} · {}", profile.name, profile.endpoint))
                    .unwrap_or_else(|| "Choose a server".into());
                let server_picker = Picker::new("vpn-server", &self.popovers, selected_label)
                    .width(Picker::REGULAR)
                    .items(profiles.iter().enumerate().map(|(index, profile)| {
                        let path = profile.path.clone();
                        let label = format!("{} · {}", profile.name, profile.endpoint);
                        MenuItem::new(format!("vpn-server-{index}"), label.clone())
                            .selected(Some(index) == selected)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_profile = Some(index);
                                this.vpn_status = vpn_engine::tunnel_status_for(&path);
                                cx.notify();
                            }))
                    }))
                    .into_any_element();
                let has_profile = selected.and_then(|index| profiles.get(index)).is_some();
                let connected = status == "Connected";
                let busy = status == "Connecting" || status == "Disconnecting";
                let transfer = if connected {
                    format!("↑ {}   ↓ {}", format_bytes(self.transfer_stats.sent_bytes), format_bytes(self.transfer_stats.received_bytes))
                } else {
                    "Traffic totals appear here while connected".to_string()
                };
                div()
                    .flex()
                    .flex_1()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_4()
                    .text_color(theme.foreground)
                    .child(div().text_size(theme.text(ui::Text::Title)).child("RosesVPN"))
                    .child(div().text_size(theme.text(ui::Text::Body)).child(if connected { "Connected · WireGuard".to_string() } else { status.clone() }))
                    .child(div().text_size(theme.text(ui::Text::Body)).text_color(theme.muted_foreground).child(transfer))
                    .child(server_picker)
                    .child(
                        div().flex().gap_3()
                            .child(Button::new("vpn-connect").primary().label("Connect").disabled(!has_profile || connected || busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.privilege_prompt = Some(PrivilegedOperation::Connect);
                                    cx.notify();
                                })))
                            .child(Button::new("vpn-disconnect").secondary().label("Disconnect").disabled(!has_profile || !connected || busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.privilege_prompt = Some(PrivilegedOperation::Disconnect);
                                    cx.notify();
                                }))),
                    )
                    .child(Button::new("vpn-import").outline().label("＋  Import WireGuard configuration")
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(path) = choose_wireguard_config() {
                                match vpn_engine::install_config(&path) {
                                    Ok(installed) => {
                                        this.profiles = vpn_engine::profiles().unwrap_or_default();
                                        this.selected_profile = this.profiles.iter().position(|profile| profile.path == installed);
                                        this.vpn_status = "Disconnected".into();
                                    }
                                    Err(error) => this.vpn_status = format!("Import failed: {error:#}"),
                                }
                                cx.notify();
                            }
                        })))
                    .child(div().text_color(theme.muted_foreground).text_size(theme.text(ui::Text::Small)).child("WireGuard tunnel with native platform firewall protection. Kill Switch is installed automatically while connecting."))
                    .into_any_element()
            },
            Page::Security => div()
                .flex().flex_1().flex_col().p_8().gap_5().w_full().max_w(px(720.))
                .child(div().text_size(theme.text(ui::Text::Title)).text_color(theme.foreground).child("Security"))
                .child(self.row("Kill Switch", "Blocks outbound traffic outside the VPN tunnel while a connection is active. It is removed after a normal disconnect.", Switch::new("kill-switch-status", self.vpn_status == "Connected").disabled(true).into_any_element(), cx))
                .child(self.row("Lockdown Mode", if cfg!(target_os = "windows") { "Keeps outbound traffic blocked after disconnect and across restarts. Requires administrator approval; switch it off to restore access." } else { "Keeps outbound traffic blocked after disconnect. Requires administrator approval; reapply it after a system restart." }, Switch::new("lockdown", self.lockdown).on_click(cx.listener(|this, _, _, cx| {
                    let enabled = !this.lockdown;
                    this.privilege_prompt = Some(PrivilegedOperation::SetLockdown(enabled));
                    cx.notify();
                })).into_any_element(), cx))
                .when_some(self.security_notice.clone(), |el, notice| el.child(div().text_color(theme.danger).text_size(theme.text(ui::Text::Small)).child(notice)))
                .child(div().text_color(theme.muted_foreground).text_size(theme.text(ui::Text::Small)).child(if cfg!(target_os = "windows") { "Windows uses Windows Filtering Platform rules. Lockdown rules persist until you turn them off; Kill Switch rules apply for the lifetime of the tunnel." } else { "Linux uses an isolated nftables output policy for the VPN endpoint, tunnel, and essential network control traffic." }))
                .into_any_element(),
            Page::Appearance => {
                let packs = icons::packs().collect::<Vec<_>>();
                let pack = packs.get(self.icon_pack % packs.len().max(1));
                let pack_label = pack.map_or_else(|| SharedString::from("Lucide"), |p| p.title());
                let theme_values = ThemeKind::ALL;
                let rounding_values = Rounding::ALL;
                let themes = Picker::new("themes", &self.popovers, self.theme.label())
                    .width(Picker::REGULAR)
                    .items(theme_values.into_iter().map(|kind| {
                        MenuItem::new(kind.id(), kind.label())
                            .selected(kind == self.theme)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.theme = kind;
                                this.apply(cx);
                                this.save();
                            }))
                    }))
                    .into_any_element();
                let icons_picker = Picker::new("packs", &self.popovers, pack_label)
                    .width(Picker::REGULAR)
                    .items(packs.iter().enumerate().map(|(index, pack)| {
                        let id = pack.id;
                        MenuItem::new(id, pack.title())
                            .selected(index == self.icon_pack)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.icon_pack = index;
                                icons::set(id);
                                cx.refresh_windows();
                                this.save();
                                cx.notify();
                            }))
                    }))
                    .into_any_element();
                let corners = Picker::new("corners", &self.popovers, self.rounding.label())
                    .width(Picker::NARROW)
                    .items(rounding_values.into_iter().map(|rounding| {
                        MenuItem::new(rounding.id(), rounding.label())
                            .selected(rounding == self.rounding)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.rounding = rounding;
                                this.apply(cx);
                                this.save();
                            }))
                    }))
                    .into_any_element();
                let blur = Switch::new("blur", self.blur)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.blur = !this.blur;
                        this.apply(cx);
                        this.save();
                    }))
                    .into_any_element();
                let blur_window = Switch::new("blur-window", self.blur_window && self.transparent)
                    .disabled(!self.transparent || !ui::WINDOW_BLUR)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.blur_window = !this.blur_window;
                        let blur_window = this.blur_window;
                        this.apply(cx);
                        window
                            .set_background_appearance(ui::backdrop(blur_window, this.transparent));
                        this.save();
                    }))
                    .into_any_element();
                let stillness = Picker::new("motion", &self.popovers, self.stillness.label())
                    .width(Picker::NARROW)
                    .items(Stillness::ALL.into_iter().map(|stillness| {
                        MenuItem::new(stillness.id(), stillness.label())
                            .selected(stillness == self.stillness)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.stillness = stillness;
                                ui::motion::apply(this.stillness, this.pace, cx);
                                this.save();
                                cx.notify();
                            }))
                    }))
                    .into_any_element();
                let pace = Picker::new("pace", &self.popovers, self.pace.label())
                    .width(Picker::NARROW)
                    .items(Pace::ALL.into_iter().map(|pace| {
                        MenuItem::new(pace.id(), pace.label())
                            .selected(pace == self.pace)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pace = pace;
                                ui::motion::apply(this.stillness, this.pace, cx);
                                this.save();
                                cx.notify();
                            }))
                    }))
                    .into_any_element();
                let ambient = Switch::new("ambient", self.ambient)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ambient = !this.ambient;
                        this.apply(cx);
                        this.save();
                        cx.notify();
                    }))
                    .into_any_element();
                let ambient_style =
                    Picker::new("ambient-style", &self.popovers, self.ambient_style.label())
                        .width(Picker::REGULAR)
                        .items(AmbientStyle::ALL.into_iter().map(|style| {
                            MenuItem::new(style.id(), style.label())
                                .selected(style == self.ambient_style)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let now = Instant::now();
                                    let from = this.ambient_colors(now);
                                    this.ambient_style = style;
                                    this.ambient_transition = ui::motion::animates(cx)
                                        .then_some(AmbientTransition { from, started: now });
                                    if ui::motion::animates(cx) {
                                        Theme::fade(this.look(), &ThemeOverrides::default(), cx);
                                    } else {
                                        this.apply(cx);
                                    }
                                    this.save();
                                    cx.notify();
                                }))
                        }))
                        .into_any_element();
                let ambient_motion = Switch::new("ambient-motion", self.ambient_motion)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ambient_motion = !this.ambient_motion;
                        this.save();
                        cx.notify();
                    }))
                    .into_any_element();
                let font = div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("font-smaller")
                            .small()
                            .outline()
                            .label("−")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.font_size = (this.font_size - 1.).max(ui::MIN_FONT);
                                this.apply(cx);
                                window.set_rem_size(cx.theme().font_size);
                                this.save();
                            })),
                    )
                    .child(
                        div()
                            .text_color(theme.foreground)
                            .child(format!("{} px", self.font_size.round())),
                    )
                    .child(
                        Button::new("font-larger")
                            .small()
                            .outline()
                            .label("+")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.font_size = (this.font_size + 1.).min(ui::MAX_FONT);
                                this.apply(cx);
                                window.set_rem_size(cx.theme().font_size);
                                this.save();
                            })),
                    )
                    .into_any_element();
                let transparency = if self.transparent {
                    self.transparency
                } else {
                    0.
                };
                let value = 1. - transparency / ui::MAX_TRANSPARENCY;
                let opacity = div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div().w(theme.metrics.cover).child(
                            Scrubber::new(&self.opacity, value)
                                .colors(theme.progress_bar, theme.muted, theme.foreground)
                                .on_move(cx.listener(|this, fraction: &f32, window, cx| {
                                    this.transparency = (1. - *fraction) * ui::MAX_TRANSPARENCY;
                                    this.transparent = this.transparency > 0.;
                                    let transparent = this.transparent;
                                    let blur_window = this.blur_window;
                                    this.apply(cx);
                                    window.set_background_appearance(ui::backdrop(
                                        blur_window,
                                        transparent,
                                    ));
                                }))
                                .on_release(cx.listener(|this, _, _, _| this.save())),
                        ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(theme.metrics.control * 1.5)
                            .whitespace_nowrap()
                            .text_right()
                            .text_color(theme.foreground)
                            .child(format!("{}%", ((1. - transparency) * 100.).round())),
                    )
                    .into_any_element();

                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .justify_center()
                            .w_full()
                            .px_6()
                            .py_3()
                            .border_b_1()
                            .border_color(theme.title_bar_border)
                            .child(
                                TabBar::new("settings-categories")
                                    .blurred()
                                    .items([Button::new("appearance-category")
                                        .label(i18n::lookup("settings-tab-appearance", None))
                                        .icon(icons::path("icons/palette.svg"))
                                        .small()
                                        .ghost()
                                        .selected(true)]),
                            ),
                    )
                    .child(
                        Scroller::new("appearance-settings", &self.scrollbar)
                            .flex_1()
                            .min_h_0()
                            .flex_col()
                            .items_center()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .w_full()
                                    .max_w(px(640.))
                                    .px_6()
                                    .py_5()
                                    .child(
                                        div()
                                            .pb_2()
                                            .text_color(theme.muted_foreground)
                                            .text_size(theme.text(ui::Text::Small))
                                            .child("Ambient"),
                                    )
                                    .child(self.row(
                                        "Ambient",
                                        "Show a soft color field behind the app",
                                        ambient,
                                        cx,
                                    ))
                                    .child(Separator::horizontal().w_full())
                                    .child(self.row(
                                        "Ambient style",
                                        "Choose the colors for the ambient field",
                                        ambient_style,
                                        cx,
                                    ))
                                    .child(Separator::horizontal().w_full())
                                    .child(self.row(
                                        "Ambient motion",
                                        "Let the color field drift slowly",
                                        ambient_motion,
                                        cx,
                                    ))
                                    .child(Separator::horizontal().w_full())
                                    .child(
                                        div()
                                            .pb_2()
                                            .text_color(theme.muted_foreground)
                                            .text_size(theme.text(ui::Text::Small))
                                            .child(i18n::lookup("settings-tab-general", None)),
                                    )
                                    .child(self.row(
                                        i18n::lookup("settings-theme", None),
                                        i18n::lookup("settings-theme-detail", None),
                                        themes,
                                        cx,
                                    ))
                                    .child(Separator::horizontal().w_full())
                                    .child(self.row(
                                        i18n::lookup("settings-icons", None),
                                        i18n::lookup("settings-icons-detail", None),
                                        icons_picker,
                                        cx,
                                    ))
                                    .child(Separator::horizontal().w_full())
                                    .child(self.row(
                                        i18n::lookup("settings-opacity", None),
                                        i18n::lookup("settings-opacity-detail", None),
                                        opacity,
                                        cx,
                                    ))
                                    .child(Separator::horizontal().w_full())
                                    .child(self.row(
                                        i18n::lookup("settings-blur", None),
                                        i18n::lookup("settings-blur-detail", None),
                                        blur,
                                        cx,
                                    ))
                                    .child(Separator::horizontal().w_full())
                                    .when(ui::WINDOW_BLUR, |this| {
                                        this.child(self.row(
                                            i18n::lookup("settings-blur-window", None),
                                            i18n::lookup("settings-blur-window-detail", None),
                                            blur_window,
                                            cx,
                                        ))
                                        .child(Separator::horizontal().w_full())
                                    })
                                    .child(self.row(
                                        i18n::lookup("settings-corners", None),
                                        i18n::lookup("settings-corners-detail", None),
                                        corners,
                                        cx,
                                    ))
                                    .child(Separator::horizontal().w_full())
                                    .child(self.row(
                                        i18n::lookup("settings-font", None),
                                        i18n::lookup("settings-font-detail", None),
                                        font,
                                        cx,
                                    ))
                                    .child(Separator::horizontal().w_full())
                                    .child(
                                        div()
                                            .pt_4()
                                            .pb_2()
                                            .text_color(theme.muted_foreground)
                                            .text_size(theme.text(ui::Text::Small))
                                            .child(i18n::lookup("settings-group-motion", None)),
                                    )
                                    .child(self.row(
                                        i18n::lookup("settings-motion", None),
                                        i18n::lookup("settings-motion-detail", None),
                                        stillness,
                                        cx,
                                    ))
                                    .child(Separator::horizontal().w_full())
                                    .child(self.row(
                                        i18n::lookup("settings-pace", None),
                                        i18n::lookup("settings-pace-detail", None),
                                        pace,
                                        cx,
                                    )),
                            ),
                    )
                    .into_any_element()
            }
        };

        let toolbar = div()
            .flex()
            .flex_none()
            .items_center()
            .w_full()
            .h(theme.metrics.title_bar)
            .px_2()
            .border_b_1()
            .border_color(theme.title_bar_border)
            .window_control_area(gpui::WindowControlArea::Drag)
            .child(
                Button::new("sidebar-toggle")
                    .ghost()
                    .small()
                    .icon(icons::path(if self.sidebar_open {
                        "icons/panel-left-close.svg"
                    } else {
                        "icons/panel-left-open.svg"
                    }))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidebar_open = !this.sidebar_open;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(gpui::WindowControlArea::Drag),
            )
            .child(WindowControls::new(false));

        let ambient_animates = self.ambient && self.ambient_motion && ui::motion::animates(cx);
        let now = Instant::now();
        let ambient_transition_active = self.ambient_transition.is_some_and(|transition| {
            ui::motion::animates(cx) && now.duration_since(transition.started) < AMBIENT_STYLE_FADE
        });
        if self.ambient_transition.is_some() && !ambient_transition_active {
            self.ambient_transition = None;
        }
        let ambient_elapsed = if ambient_animates {
            now.duration_since(self.ambient_started).as_secs_f32()
        } else {
            0.
        };
        if ambient_animates || ambient_transition_active {
            window.request_animation_frame();
        }
        let bounds = self.ambient_bounds.get();
        let ambient_width = bounds.size.width.as_f32().max(1.) / AMBIENT_DOWNSCALE;
        let ambient_height = bounds.size.height.as_f32().max(1.) / AMBIENT_DOWNSCALE;
        let ambient_colors = self.ambient_colors(now);
        let ambient_bounds = self.ambient_bounds.clone();
        let ambient_opacity = if theme.transparent {
            theme.background.a
        } else {
            1.
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .track_focus(&self.focus)
            .when(self.ambient, |root| {
                root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .overflow_hidden()
                        .opacity(ambient_opacity)
                        .child(
                            canvas(
                                {
                                    let bounds = ambient_bounds.clone();
                                    move |measured, _, _| bounds.set(measured)
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .w(px(ambient_width))
                                .h(px(ambient_height))
                                .layer_scale(AMBIENT_DOWNSCALE)
                                .layer_scale_origin(point(0., 0.))
                                .blur(px(AMBIENT_BLUR / window.scale_factor()))
                                .children(AMBIENT_SPECS.into_iter().enumerate().map(
                                    |(index, spec)| {
                                        let (base_x, base_y, size, period, phase, amp_x, amp_y) =
                                            spec;
                                        let spin = TAU * ambient_elapsed / period;
                                        let x = base_x + amp_x * (spin + phase).sin();
                                        let y = base_y + amp_y * (spin * 0.83 + phase * 1.7).cos();
                                        let diameter = ambient_width.min(ambient_height) * size;
                                        let color = ambient_colors[index];
                                        div()
                                            .absolute()
                                            .left(px(x * ambient_width - diameter / 2.))
                                            .top(px(y * ambient_height - diameter / 2.))
                                            .size(px(diameter))
                                            .children((0..AMBIENT_DISCS).map(move |step| {
                                                let fraction = 1.
                                                    - step as f32 / AMBIENT_DISCS as f32
                                                        * (1. - 1. / AMBIENT_DISCS as f32);
                                                let opacity = 0.027
                                                    + step as f32 / (AMBIENT_DISCS as f32 - 1.)
                                                        * (0.079 - 0.027);
                                                let disc = diameter * fraction;
                                                div()
                                                    .absolute()
                                                    .left(px((diameter - disc) / 2.))
                                                    .top(px((diameter - disc) / 2.))
                                                    .size(px(disc))
                                                    .rounded_full()
                                                    .bg(shaded(color).opacity(opacity))
                                            }))
                                    },
                                )),
                        ),
                )
            })
            .child(toolbar)
            .child({
                let sidebar = div().flex_none().h_full().overflow_hidden().child(sidebar);
                let sidebar = if ui::motion::animates(cx) {
                    sidebar
                        .motion(
                            ("sidebar-width", usize::from(sidebar_open)),
                            Motion::Slow,
                            move |sidebar, progress| {
                                let width = if sidebar_open {
                                    208. * progress
                                } else {
                                    208. * (1. - progress)
                                };
                                sidebar.w(px(width.max(0.)))
                            },
                        )
                        .into_any_element()
                } else {
                    sidebar
                        .w(px(if sidebar_open { 208. } else { 0. }))
                        .into_any_element()
                };

                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(content)
            })
            .when_some(self.privilege_prompt, |root, operation| {
                root.child(
                    ui::Modal::new("administrator-access", "Administrator access")
                        .detail("RosesVPN needs permission to change your network configuration.")
                        .child(div().text_color(theme.foreground).child(
                            "The next step asks Windows for administrator approval or opens your Linux authentication prompt. RosesVPN uses this access only to set up or remove its encrypted tunnel and network protection rules. Canceling keeps your current network settings unchanged.",
                        ))
                        .action(
                            Button::new("administrator-cancel")
                                .outline()
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.privilege_prompt = None;
                                    cx.notify();
                                })),
                        )
                        .action(
                            Button::new("administrator-continue")
                                .primary()
                                .label("Continue")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.run_privileged_operation(operation, cx);
                                    cx.notify();
                                })),
                        )
                        .on_dismiss(cx.listener(|this, _, _, cx| {
                            this.privilege_prompt = None;
                            cx.notify();
                        })),
                )
            })
    }
}

fn shaded(color: Hsla) -> Hsla {
    let rgba = Rgba::from(color);
    let kept = 0.45;
    Hsla::from(Rgba {
        r: rgba.r * kept,
        g: rgba.g * kept,
        b: rgba.b * kept,
        a: rgba.a,
    })
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(std::ffi::OsStr::new("--admin-helper")) {
        if let Err(error) = vpn_engine::run_admin_helper(args) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    env_logger::init();
    i18n::set(i18n::resolve("en"));
    let prefs = Preferences::load();
    icons::set(prefs.icon_pack);

    gpui_platform::application()
        .with_assets(icons::Assets)
        .run(move |cx: &mut App| {
            let look = Look {
                kind: prefs.theme,
                rounding: prefs.rounding,
                font: prefs.font_size,
                transparent: prefs.transparent,
                transparency: prefs.transparency,
                blur: prefs.blur,
                blur_window: prefs.blur_window,
                tint: prefs.ambient.then(|| prefs.ambient_style.colors().0),
                tint_secondary: prefs.ambient.then(|| prefs.ambient_style.colors().1),
            };
            Theme::init(look, &ThemeOverrides::default(), cx);
            ui::motion::apply(prefs.stillness, prefs.pace, cx);
            let bounds = WindowBounds::Windowed(Bounds::centered(None, FIRST_SIZE, cx));
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(bounds),
                    window_background: ui::backdrop(prefs.blur_window, prefs.transparent),
                    titlebar: Some(TitlebarOptions {
                        title: Some("RosesVPN".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    app_owns_titlebar_drag: true,
                    window_min_size: Some(gpui::size(px(480.), px(400.))),
                    app_id: Some("rosesvpn".into()),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_rem_size(cx.theme().font_size);
                    let view = cx.new(|cx| RosesVpn::new(prefs, cx));
                    view.update(cx, |this, cx| {
                        this.appearance_observer =
                            Some(cx.observe_window_appearance(window, |this, window, cx| {
                                let reported = ThemeKind::reported(cx);
                                let changed = ThemeKind::assumed() != Some(reported);
                                if changed {
                                    ThemeKind::assume(reported);
                                }
                                if this.theme == ThemeKind::System && changed {
                                    Theme::fade(this.look(), &ThemeOverrides::default(), cx);
                                }
                                window.set_background_appearance(ui::backdrop(
                                    this.blur_window,
                                    this.transparent,
                                ));
                            }));
                    });
                    view
                },
            )
            .expect("failed to open window");
            cx.activate(true);
        });
}

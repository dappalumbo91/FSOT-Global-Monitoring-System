#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! FSOT Global Monitor. Interactive Earth plus the live feeds.

mod camera;
mod feeds;
mod geo;
mod labels;
mod layers;
mod mesh;
mod overlay;
mod record;
mod renderer;
mod series;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use camera::Camera;
use feeds::{FeedState, SharedFeeds};
use image::RgbaImage;
use labels::{LabelKind, ScreenLabel};
use layers::Layers;
use overlay::DynLabel;
use renderer::{FrameResult, GlobeRenderer, OverlayKind};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,fsot_global_monitor=info"))
        .init();
    std::panic::set_hook(Box::new(|info| {
        let message = info.to_string();
        eprintln!("{message}");
        alert(&message);
    }));
    if std::env::args().any(|arg| arg == "--check") {
        let report = feeds::accuracy_report();
        let path = renderer::project_path("cache/accuracy-check.txt");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, &report);
        println!("{report}");
        println!("wrote {}", path.display());
        if report.lines().any(|line| line.starts_with("FAIL")) {
            std::process::exit(1);
        }
        return Ok(());
    }
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(std::env::args().any(|arg| arg == "--screenshot"));
    event_loop.run_app(&mut app)?;
    Ok(())
}

struct Pointer {
    pos: [f32; 2],
    press_pos: [f32; 2],
    left: bool,
    right: bool,
    suppress_left: bool,
    last_click: Option<(Instant, [f32; 2])>,
    vel: [f32; 2],
    last_move: Instant,
}

impl Pointer {
    fn new() -> Self {
        Self {
            pos: [0.0, 0.0],
            press_pos: [0.0, 0.0],
            left: false,
            right: false,
            suppress_left: false,
            last_click: None,
            vel: [0.0, 0.0],
            last_move: Instant::now(),
        }
    }
}

struct Readout {
    cursor: String,
    altitude: String,
    heading: String,
    tilt: String,
    utc: String,
    sun: String,
    fps: String,
}

struct App {
    screenshot: bool,
    shot_path: PathBuf,
    started: Instant,
    window: Option<Arc<Window>>,
    gpu: Option<GlobeRenderer>,
    egui_state: Option<egui_winit::State>,
    egui_ctx: egui::Context,
    camera: Camera,
    home: Camera,
    layers: Layers,
    feeds: SharedFeeds,
    local: FeedState,
    seen_generation: u64,
    pointer: Pointer,
    last_frame: Instant,
    fps: f32,
    unix: f64,
    view_px: [u32; 2],
    presented: u32,
    attempts: u32,
    accepted_clicks: u32,
    panel: PanelState,
    flight: Option<Flight>,
    place_box: Arc<Mutex<PlaceBox>>,
    ensemble_box: Arc<Mutex<EnsembleBox>>,
    air_box: Arc<Mutex<AirBox>>,
    geocode_box: Arc<Mutex<GeocodeSlot>>,
    geocode_wait: Option<(String, Instant)>,
    desk_open: bool,
    chrome: Chrome,
}

impl App {
    fn new(screenshot: bool) -> Self {
        let unix = geo::unix_now();
        let camera = Camera::sunlit(unix);
        let ctx = egui::Context::default();
        style_ui(&ctx);
        install_ui_font(&ctx);
        let feeds = feeds::start();
        let local = feeds.lock().unwrap_or_else(|err| err.into_inner()).clone();
        Self {
            screenshot,
            shot_path: renderer::project_path("shots/globe.png"),
            started: Instant::now(),
            window: None,
            gpu: None,
            egui_state: None,
            egui_ctx: ctx,
            camera,
            home: camera,
            layers: Layers::default(),
            feeds,
            local,
            seen_generation: 0,
            pointer: Pointer::new(),
            last_frame: Instant::now(),
            fps: 0.0,
            unix,
            view_px: [1440, 900],
            presented: 0,
            attempts: 0,
            accepted_clicks: 0,
            panel: PanelState::load(),
            flight: None,
            place_box: Arc::new(Mutex::new(PlaceBox { key: String::new(), result: None })),
            ensemble_box: Arc::new(Mutex::new(EnsembleBox { key: String::new(), result: None })),
            air_box: Arc::new(Mutex::new(AirBox { key: String::new(), result: None })),
            geocode_box: Arc::new(Mutex::new(GeocodeSlot { key: String::new(), result: None })),
            geocode_wait: None,
            desk_open: screenshot,
            chrome: Chrome::at_launch(),
        }
    }

    fn sync_feeds(&mut self) {
        let fresh = {
            let guard = self.feeds.lock().unwrap_or_else(|err| err.into_inner());
            if guard.generation == self.seen_generation {
                None
            } else {
                Some(guard.clone())
            }
        };
        if let Some(fresh) = fresh {
            self.seen_generation = fresh.generation;
            self.local = fresh;
            self.upload_static_overlays();
        }
        if self.layers.weather {
            if let Some(gpu) = self.gpu.as_mut() {
                let mut mesh = overlay::wind_streaks(&self.local.wind, self.unix);
                overlay::append_mesh(&mut mesh, overlay::storm_markers(&self.local.bulletin.storms));
                gpu.set_overlay(OverlayKind::Weather, &mesh);
            }
        }
    }

    fn upload_static_overlays(&mut self) {
        let faults = overlay::fault_lines(&self.local.faults);
        let mut volcanoes = overlay::volcano_markers(&self.local.volcanoes, &self.local.bulletin.volcano_alerts);
        overlay::append_mesh(&mut volcanoes, overlay::weekly_markers(&self.local.bulletin.weekly));
        let quakes = overlay::quake_markers(&self.local.quakes);
        let mut ocean = overlay::ocean_markers(&self.local.ocean);
        overlay::append_mesh(&mut ocean, overlay::current_streaks(&self.local.ocean));
        let Some(gpu) = self.gpu.as_mut() else { return };
        gpu.set_overlay(OverlayKind::Faults, &faults);
        gpu.set_overlay(OverlayKind::Volcanoes, &volcanoes);
        gpu.set_overlay(OverlayKind::Quakes, &quakes);
        gpu.set_overlay(OverlayKind::Ocean, &ocean);
        if let Some((width, height, rgba)) = overlay::elevation_rgba(&self.local.elevation) {
            gpu.set_elevation(width, height, &rgba);
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let real = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        if real > 0.0 {
            let inst = 1.0 / real;
            self.fps = if self.fps == 0.0 { inst } else { self.fps * 0.92 + inst * 0.08 };
        }
        let dt = real.clamp(0.0, 0.05);
        self.unix = geo::unix_now();
        self.poll_search();
        let cancel_flight = self.pointer.left || self.pointer.right;
        let flight_done = if cancel_flight { false } else { self.step_flight(dt) };
        if cancel_flight || flight_done {
            self.flight = None;
        }
        if !self.pointer.left && !self.pointer.right {
            let h = self.view_px[1].max(1) as f32;
            self.camera.spin_pixels(self.pointer.vel[0] * dt, self.pointer.vel[1] * dt, h);
            let decay = (-3.2 * dt).exp();
            self.pointer.vel[0] *= decay;
            self.pointer.vel[1] *= decay;
            if self.pointer.vel[0].abs() < 2.0 && self.pointer.vel[1].abs() < 2.0 {
                self.pointer.vel = [0.0, 0.0];
            }
        }
        self.camera.ease_tilt(dt);
    }

    fn step_flight(&mut self, dt: f32) -> bool {
        let Some(flight) = self.flight.as_mut() else { return false };
        flight.t = (flight.t + dt / 0.72).min(1.0);
        let s = flight.t * flight.t * (3.0 - 2.0 * flight.t);
        self.camera.lat = flight.from_lat + (flight.to_lat - flight.from_lat) * s;
        self.camera.lon = geo::wrap_pi(flight.from_lon + flight.dlon * s);
        self.camera.height = (flight.from_h + (flight.to_h - flight.from_h) * s).clamp(camera::MIN_HEIGHT, camera::MAX_HEIGHT);
        flight.t >= 1.0
    }

    fn fly_to(&mut self, lat_deg: f32, lon_deg: f32) {
        let to_lat = lat_deg.to_radians();
        let to_lon = geo::wrap_pi(lon_deg.to_radians());
        let from_h = self.camera.height;
        let to_h = if from_h > 0.85 { 0.42 } else { from_h.max(0.1) };
        self.flight = Some(Flight {
            from_lat: self.camera.lat,
            from_lon: self.camera.lon,
            to_lat,
            dlon: geo::wrap_pi(to_lon - self.camera.lon),
            from_h,
            to_h,
            t: 0.0,
        });
        self.pointer.vel = [0.0, 0.0];
    }

    fn choose_spot(&mut self, spot: Spot, fly: bool) {
        if fly {
            self.fly_to(spot.lat, spot.lon);
        }
        self.ask_hours(spot.lat, spot.lon);
        self.panel.spot = Some(spot);
        self.panel.query.clear();
        self.panel.submit = false;
    }

    fn ask_hours(&mut self, lat: f32, lon: f32) {
        let key = format!("{lat:.3},{lon:.3}");
        {
            let mut slot = self.place_box.lock().unwrap_or_else(|err| err.into_inner());
            if slot.key == key {
                return;
            }
            slot.key = key.clone();
            slot.result = None;
            let mut ensemble = self.ensemble_box.lock().unwrap_or_else(|err| err.into_inner());
            ensemble.key = key.clone();
            ensemble.result = None;
            let mut air = self.air_box.lock().unwrap_or_else(|err| err.into_inner());
            air.key = key.clone();
            air.result = None;
        }
        let slot = Arc::clone(&self.place_box);
        let ensemble_slot = Arc::clone(&self.ensemble_box);
        let air_slot = Arc::clone(&self.air_box);
        thread::spawn(move || {
            let result = feeds::fetch_place_hours(lat, lon);
            {
                let mut slot = slot.lock().unwrap_or_else(|err| err.into_inner());
                if slot.key == key {
                    slot.result = Some(result);
                }
            }
            let result = feeds::fetch_place_ensemble(lat, lon);
            {
                let mut ensemble = ensemble_slot.lock().unwrap_or_else(|err| err.into_inner());
                if ensemble.key == key {
                    ensemble.result = Some(result);
                }
            }
            let result = feeds::fetch_place_air(lat, lon);
            let mut air = air_slot.lock().unwrap_or_else(|err| err.into_inner());
            if air.key == key {
                air.result = Some(result);
            }
        });
    }

    fn place_forecast(&self) -> Option<Result<feeds::PlaceForecast, String>> {
        self.place_box.lock().unwrap_or_else(|err| err.into_inner()).result.clone()
    }

    fn place_ensemble(&self) -> Option<Result<feeds::PlaceEnsemble, String>> {
        self.ensemble_box.lock().unwrap_or_else(|err| err.into_inner()).result.clone()
    }

    fn place_air(&self) -> Option<Result<feeds::PlaceAir, String>> {
        self.air_box.lock().unwrap_or_else(|err| err.into_inner()).result.clone()
    }

    fn geocode_view(&self) -> Option<Result<Vec<feeds::GeoHit>, String>> {
        let query = self.panel.query.trim();
        if query.chars().count() < 2 {
            return None;
        }
        let slot = self.geocode_box.lock().unwrap_or_else(|err| err.into_inner());
        if slot.key == query { slot.result.clone() } else { None }
    }

    fn poll_search(&mut self) {
        let query = self.panel.query.trim().to_string();
        if query.chars().count() < 2 {
            let mut slot = self.geocode_box.lock().unwrap_or_else(|err| err.into_inner());
            slot.key.clear();
            slot.result = None;
            self.geocode_wait = None;
            return;
        }
        let pending = {
            let slot = self.geocode_box.lock().unwrap_or_else(|err| err.into_inner());
            slot.key == query
        };
        if pending {
            self.geocode_wait = None;
            return;
        }
        match &self.geocode_wait {
            Some((waiting, since)) if waiting == &query && since.elapsed() >= Duration::from_millis(350) => {
                let query = query.clone();
                self.geocode_wait = None;
                self.ask_geocode(query);
            }
            Some((waiting, _)) if waiting == &query => {}
            _ => self.geocode_wait = Some((query, Instant::now())),
        }
    }

    fn ask_geocode(&mut self, query: String) {
        {
            let mut slot = self.geocode_box.lock().unwrap_or_else(|err| err.into_inner());
            slot.key = query.clone();
            slot.result = None;
        }
        let slot = Arc::clone(&self.geocode_box);
        thread::spawn(move || {
            let result = feeds::fetch_geocode(&query);
            let mut slot = slot.lock().unwrap_or_else(|err| err.into_inner());
            if slot.key == query {
                slot.result = Some(result);
            }
        });
    }

    fn select_click(&mut self) {
        let w = self.view_px[0].max(1) as f32;
        let h = self.view_px[1].max(1) as f32;
        let Some(hit) = self.camera.pick(self.pointer.pos[0], self.pointer.pos[1], w, h) else { return };
        let (lat, lon) = geo::dir_to_lat_lon(hit);
        let lat = lat.to_degrees();
        let lon = lon.to_degrees();
        self.choose_spot(Spot { name: geo::format_lat_lon_deg(lat, lon), lat, lon }, false);
    }

    fn apply_panel(&mut self, command: PanelCmd) {
        match command {
            PanelCmd::Choose(spot) => self.choose_spot(spot, true),
            PanelCmd::Save => self.save_spot(false),
            PanelCmd::MakeHome => self.save_spot(true),
            PanelCmd::OpenSaved(index) => {
                let Some(place) = self.panel.saved.get(index).cloned() else { return };
                self.choose_spot(Spot { name: place.name, lat: place.lat, lon: place.lon }, true);
            }
        }
    }

    fn save_spot(&mut self, home: bool) {
        let Some(spot) = self.panel.spot.clone() else { return };
        if home {
            for place in &mut self.panel.saved {
                place.home = false;
            }
        }
        if let Some(found) = self.panel.saved.iter_mut().find(|place| (place.lat - spot.lat).abs() < 0.05 && (place.lon - spot.lon).abs() < 0.05) {
            found.name = spot.name;
            if home {
                found.home = true;
            }
        } else {
            if self.panel.saved.len() >= 8 {
                if let Some(index) = self.panel.saved.iter().position(|place| !place.home) {
                    self.panel.saved.remove(index);
                }
            }
            self.panel.saved.push(SavedPlace { name: spot.name, lat: spot.lat, lon: spot.lon, home });
        }
        self.panel.store();
    }

    fn pan(&mut self, prev: [f32; 2], curr: [f32; 2]) {
        let w = self.view_px[0].max(1) as f32;
        let h = self.view_px[1].max(1) as f32;
        match (
            self.camera.pick(prev[0], prev[1], w, h),
            self.camera.pick(curr[0], curr[1], w, h),
        ) {
            (Some(a), Some(b)) => self.camera.grab(a, b),
            _ => self.camera.spin_pixels(curr[0] - prev[0], curr[1] - prev[1], h),
        }
    }

    fn readout(&self) -> Readout {
        let w = self.view_px[0].max(1) as f32;
        let h = self.view_px[1].max(1) as f32;
        let cursor = match self.camera.pick(self.pointer.pos[0], self.pointer.pos[1], w, h) {
            Some(hit) => {
                let (lat, lon) = geo::dir_to_lat_lon(hit);
                let mut text = geo::format_lat_lon_deg(lat.to_degrees(), lon.to_degrees());
                if self.layers.elevation {
                    if let Some(meters) = self.local.elevation.sample(lat.to_degrees(), lon.to_degrees()) {
                        text.push_str(&format!("  ·  {meters} m"));
                    }
                }
                text
            }
            None => "—".to_string(),
        };
        let (sun_lat, sun_lon) = geo::subsolar_lat_lon(self.unix);
        Readout {
            cursor,
            altitude: format_km(self.camera.height * geo::EARTH_RADIUS_KM),
            heading: format!("{:.0}°", self.camera.heading.to_degrees().rem_euclid(360.0)),
            tilt: format!("{:.0}°", self.camera.tilt.to_degrees()),
            utc: geo::format_unix_utc(self.unix),
            sun: geo::format_lat_lon_deg(sun_lat.to_degrees() as f32, sun_lon.to_degrees() as f32),
            fps: format!("{:.0}", self.fps),
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let Some(window) = self.window.clone() else { return };
        let Some(_) = self.gpu.as_ref() else { return };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.view_px = [size.width, size.height];
        self.tick();
        self.sync_feeds();

        let camera = self.camera;
        let ppp = window.scale_factor() as f32;
        let labels = labels::project_labels(&camera, size.width as f32, size.height as f32);
        let feed_labels = overlay::project_feed_labels(
            &camera,
            size.width as f32,
            size.height as f32,
            &self.local.cities,
            &self.local.quakes,
            &self.local.volcanoes,
            &self.local.bulletin.volcano_alerts,
            &self.local.bulletin.storms,
            &self.local.bulletin.weekly,
            self.layers.cities,
            self.layers.quakes,
            self.layers.volcanoes,
            self.layers.weather,
        );
        let readout = self.readout();
        let forecast = self.place_forecast();
        let ensemble = self.place_ensemble();
        let air = self.place_air();
        let geocode = self.geocode_view();
        let ctx = self.egui_ctx.clone();
        let input = self.egui_state.as_mut().unwrap().take_egui_input(&window);
        let mut command = None;
        let mut chrome = Chrome::none();
        let mut desk_open = self.desk_open;
        let full = ctx.run_ui(input, |ui| {
            command = hud(
                ui,
                &mut self.layers,
                &self.local,
                &mut self.panel,
                &forecast,
                &ensemble,
                &air,
                &geocode,
                &readout,
                &labels,
                &feed_labels,
                ppp,
                &mut desk_open,
                &mut chrome,
            );
        });
        self.desk_open = desk_open;
        self.chrome = chrome;
        if let Some(command) = command {
            self.apply_panel(command);
        }
        let egui::FullOutput {
            platform_output,
            mut textures_delta,
            shapes,
            pixels_per_point,
            ..
        } = full;
        self.egui_state
            .as_mut()
            .unwrap()
            .handle_platform_output(&window, platform_output);
        let primitives = ctx.tessellate(shapes, pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point,
        };
        let settings = self.layers.frame(camera.height);
        let sun = geo::sun_direction(self.unix);
        let capture = self.screenshot && self.presented >= 3;

        self.attempts += 1;
        let gpu = self.gpu.as_mut().unwrap();
        let result = gpu.render(
            &camera,
            sun,
            &settings,
            &textures_delta,
            &primitives,
            &screen,
            capture,
        );
        textures_delta.clear();
        match result {
            Ok(FrameResult::Drawn(image)) => {
                self.presented += 1;
                if let Some(image) = image {
                    let feeds_ready = self.local.live_done || self.started.elapsed().as_secs() > 240;
                    if feeds_ready && (frame_has_detail(&image) || self.presented > 40) {
                        let desk = feeds::desk_text(
                            &self.local.quakes,
                            &self.local.bulletin,
                            self.panel.spot.is_some(),
                            &self.place_ensemble(),
                            &self.place_air(),
                        );
                        let _ = std::fs::write(renderer::project_path("shots/desk.txt"), desk);
                        match image.save(&self.shot_path) {
                            Ok(()) => println!("saved {}", self.shot_path.display()),
                            Err(err) => eprintln!("could not save {}: {err}", self.shot_path.display()),
                        }
                        event_loop.exit();
                        return;
                    }
                }
            }
            Ok(FrameResult::Lost) => {
                if let Err(err) = self.gpu.as_mut().unwrap().recreate_surface(&window) {
                    eprintln!("surface lost: {err}");
                    event_loop.exit();
                    return;
                }
            }
            Ok(FrameResult::Retry) => {
                if self.screenshot && self.attempts > 180 && self.presented == 0 {
                    eprintln!("no frame was presented");
                    event_loop.exit();
                    return;
                }
            }
            Err(err) => {
                eprintln!("render: {err}");
                if self.screenshot {
                    event_loop.exit();
                    return;
                }
            }
        }
        window.request_redraw();
        self.note_control();
    }

    /// Clicks on the search bar, the monitor column, the weather card, the reading, or the FSOT desk stay off the globe.
    fn on_globe(&self) -> bool {
        let scale = self.window.as_ref().map(|window| window.scale_factor() as f32).unwrap_or(1.0);
        let point = egui::pos2(self.pointer.pos[0] / scale, self.pointer.pos[1] / scale);
        !self.chrome.blocks(point)
    }

    fn note_control(&self) {
        if std::env::var_os("FSOT_CONTROL_TEST").is_none() {
            return;
        }
        let line = format!(
            "{:.5} {:.5} {:.5} {:.5} {} {:.1} {:.1}\n",
            self.camera.lat.to_degrees(),
            self.camera.lon.to_degrees(),
            self.camera.height,
            self.camera.tilt.to_degrees(),
            self.accepted_clicks,
            self.pointer.pos[0],
            self.pointer.pos[1],
        );
        let _ = std::fs::write(renderer::project_path("shots/control-state.txt"), line);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let title = if std::env::var_os("FSOT_CONTROL_TEST").is_some() {
            "FSOT Global Monitor Check"
        } else {
            "FSOT Global Monitor"
        };
        let attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .with_min_inner_size(LogicalSize::new(960.0, 640.0));
        let window = match event_loop.create_window(attrs) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                let message = format!("create window: {err}");
                eprintln!("{message}");
                alert(&message);
                event_loop.exit();
                return;
            }
        };
        let gpu = match GlobeRenderer::new(&window) {
            Ok(gpu) => gpu,
            Err(err) => {
                let message = format!("FSOT Global Monitor failed to start: {err}");
                eprintln!("{message}");
                alert(&message);
                event_loop.exit();
                return;
            }
        };
        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        self.view_px = [window.inner_size().width.max(1), window.inner_size().height.max(1)];
        self.egui_state = Some(egui_state);
        self.gpu = Some(gpu);
        self.window = Some(window.clone());
        if self.screenshot {
            if let Some(parent) = self.shot_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else { return };
        if let Some(state) = self.egui_state.as_mut() {
            // egui still sees the event so the panel can scroll and toggle.
            // Navigation uses the pointer position, because a full-window panel
            // makes egui report every click as its own.
            let _ = state.on_window_event(&window, &event);
        }
        let globe = self.on_globe();

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.view_px = [size.width, size.height];
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            WindowEvent::CursorMoved { position, .. } => {
                let pos = [position.x as f32, position.y as f32];
                let prev = self.pointer.pos;
                let now = Instant::now();
                let dt = now.duration_since(self.pointer.last_move).as_secs_f32().clamp(1e-4, 0.05);
                if self.pointer.left && !self.pointer.suppress_left {
                    self.pan(prev, pos);
                    self.pointer.vel = [((pos[0] - prev[0]) / dt).clamp(-1800.0, 1800.0), ((pos[1] - prev[1]) / dt).clamp(-1800.0, 1800.0)];
                }
                if self.pointer.right {
                    self.camera.orbit(pos[0] - prev[0], pos[1] - prev[1]);
                    self.pointer.vel = [0.0, 0.0];
                }
                self.pointer.pos = pos;
                self.pointer.last_move = now;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                match button {
                    MouseButton::Left => self.on_left(pressed, globe),
                    MouseButton::Right => {
                        if pressed && globe {
                            self.pointer.right = true;
                            self.pointer.vel = [0.0, 0.0];
                        } else if !pressed {
                            self.pointer.right = false;
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } if globe => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                let w = self.view_px[0].max(1) as f32;
                let h = self.view_px[1].max(1) as f32;
                let hit = self.camera.pick(self.pointer.pos[0], self.pointer.pos[1], w, h);
                self.camera.zoom(steps, hit);
                self.pointer.vel[0] *= 0.4;
                self.pointer.vel[1] *= 0.4;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    let h = self.view_px[1].max(1) as f32;
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) if !event.repeat => event_loop.exit(),
                        Key::Character(text) if !event.repeat && text.eq_ignore_ascii_case("r") && !self.egui_ctx.egui_wants_keyboard_input() => {
                            self.camera = self.home;
                            self.flight = None;
                            self.pointer.vel = [0.0, 0.0];
                        }
                        Key::Named(NamedKey::ArrowLeft) if !self.egui_ctx.egui_wants_keyboard_input() => self.camera.spin_pixels(-36.0, 0.0, h),
                        Key::Named(NamedKey::ArrowRight) if !self.egui_ctx.egui_wants_keyboard_input() => self.camera.spin_pixels(36.0, 0.0, h),
                        Key::Named(NamedKey::ArrowUp) if !self.egui_ctx.egui_wants_keyboard_input() => self.camera.spin_pixels(0.0, -36.0, h),
                        Key::Named(NamedKey::ArrowDown) if !self.egui_ctx.egui_wants_keyboard_input() => self.camera.spin_pixels(0.0, 36.0, h),
                        Key::Character(text) if !self.egui_ctx.egui_wants_keyboard_input() && (text == "+" || text == "=") => self.camera.zoom(1.0, None),
                        Key::Character(text) if !self.egui_ctx.egui_wants_keyboard_input() && text == "-" => self.camera.zoom(-1.0, None),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl App {
    fn on_left(&mut self, pressed: bool, globe: bool) {
        if !pressed {
            let was_click = self.pointer.left && !self.pointer.suppress_left && globe;
            let moved = (self.pointer.press_pos[0] - self.pointer.pos[0]).abs() + (self.pointer.press_pos[1] - self.pointer.pos[1]).abs();
            self.pointer.left = false;
            self.pointer.suppress_left = false;
            if was_click && moved < 10.0 {
                self.select_click();
            }
            return;
        }
        if !globe {
            return;
        }
        self.pointer.press_pos = self.pointer.pos;
        self.accepted_clicks = self.accepted_clicks.saturating_add(1);
        let now = Instant::now();
        let double = self.pointer.last_click.is_some_and(|(t, p)| {
            now.duration_since(t).as_millis() < 350
                && (p[0] - self.pointer.pos[0]).abs() < 8.0
                && (p[1] - self.pointer.pos[1]).abs() < 8.0
        });
        if double {
            let w = self.view_px[0].max(1) as f32;
            let h = self.view_px[1].max(1) as f32;
            let hit = self.camera.pick(self.pointer.pos[0], self.pointer.pos[1], w, h);
            self.camera.zoom_double(hit);
            self.pointer.suppress_left = true;
            self.pointer.vel = [0.0, 0.0];
            self.pointer.last_click = None;
        } else {
            self.pointer.left = true;
            self.pointer.suppress_left = false;
            self.pointer.vel = [0.0, 0.0];
            self.pointer.last_click = Some((now, self.pointer.pos));
        }
    }
}

#[derive(Clone)]
struct Spot {
    name: String,
    lat: f32,
    lon: f32,
}

struct Flight {
    from_lat: f32,
    from_lon: f32,
    to_lat: f32,
    dlon: f32,
    from_h: f32,
    to_h: f32,
    t: f32,
}

#[derive(Clone)]
struct SavedPlace {
    name: String,
    lat: f32,
    lon: f32,
    home: bool,
}

struct PanelState {
    query: String,
    submit: bool,
    spot: Option<Spot>,
    saved: Vec<SavedPlace>,
    units: feeds::ShowUnits,
}

struct PlaceBox {
    key: String,
    result: Option<Result<feeds::PlaceForecast, String>>,
}

struct EnsembleBox {
    key: String,
    result: Option<Result<feeds::PlaceEnsemble, String>>,
}

struct AirBox {
    key: String,
    result: Option<Result<feeds::PlaceAir, String>>,
}

enum PanelCmd {
    Choose(Spot),
    Save,
    MakeHome,
    OpenSaved(usize),
}

impl PanelState {
    fn load() -> Self {
        let mut panel = Self {
            query: String::new(),
            submit: false,
            spot: None,
            saved: Vec::new(),
            units: feeds::ShowUnits::default(),
        };
        let Ok(text) = std::fs::read_to_string(renderer::project_path("cache/places.json")) else { return panel };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return panel };
        let Some(list) = value.get("saved").and_then(|item| item.as_array()) else { return panel };
        for item in list {
            let Some(name) = item.get("name").and_then(|v| v.as_str()) else { continue };
            let Some(lat) = item.get("lat").and_then(|v| v.as_f64()) else { continue };
            let Some(lon) = item.get("lon").and_then(|v| v.as_f64()) else { continue };
            panel.saved.push(SavedPlace {
                name: name.to_string(),
                lat: lat as f32,
                lon: lon as f32,
                home: item.get("home").and_then(|v| v.as_bool()).unwrap_or(false),
            });
        }
        panel
    }

    fn store(&self) {
        let saved: Vec<serde_json::Value> = self
            .saved
            .iter()
            .map(|place| serde_json::json!({ "name": place.name, "lat": place.lat, "lon": place.lon, "home": place.home }))
            .collect();
        let text = serde_json::to_string(&serde_json::json!({ "saved": saved })).unwrap_or_else(|_| "{}".into());
        let path = renderer::project_path("cache/places.json");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, text);
    }
}

struct GeocodeSlot {
    key: String,
    result: Option<Result<Vec<feeds::GeoHit>, String>>,
}

struct Chrome {
    top: egui::Rect,
    left: egui::Rect,
    right: egui::Rect,
    bottom: egui::Rect,
    desk: egui::Rect,
}

impl Chrome {
    fn none() -> Self {
        Self {
            top: egui::Rect::NOTHING,
            left: egui::Rect::NOTHING,
            right: egui::Rect::NOTHING,
            bottom: egui::Rect::NOTHING,
            desk: egui::Rect::NOTHING,
        }
    }

    fn at_launch() -> Self {
        Self {
            top: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(4000.0, 64.0)),
            left: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(276.0, 4000.0)),
            right: egui::Rect::NOTHING,
            bottom: egui::Rect::NOTHING,
            desk: egui::Rect::NOTHING,
        }
    }

    fn blocks(&self, point: egui::Pos2) -> bool {
        self.top.contains(point)
            || self.left.contains(point)
            || self.right.contains(point)
            || self.bottom.contains(point)
            || self.desk.contains(point)
    }
}

fn panel_frame() -> egui::Frame {
    egui::Frame::NONE.fill(egui::Color32::from_rgba_unmultiplied(6, 10, 18, 216)).inner_margin(egui::Margin::same(12))
}

fn take_cmd(slot: &mut Option<PanelCmd>, next: Option<PanelCmd>) {
    if next.is_some() {
        *slot = next;
    }
}

fn matching_places(feeds: &FeedState, query: &str, geocode: &Option<Result<Vec<feeds::GeoHit>, String>>) -> Vec<Spot> {
    if let Some(Ok(hits)) = geocode {
        if !hits.is_empty() {
            return hits.iter().map(|hit| Spot { name: hit.label(), lat: hit.lat, lon: hit.lon }).collect();
        }
    }
    feeds::search_cities(&feeds.cities, query)
        .into_iter()
        .map(|city| Spot { name: city.name.clone(), lat: city.lat, lon: city.lon })
        .collect()
}

fn search_bar(
    ui: &mut egui::Ui,
    feeds: &FeedState,
    panel: &mut PanelState,
    geocode: &Option<Result<Vec<feeds::GeoHit>, String>>,
    desk_open: &mut bool,
    muted: egui::Color32,
) -> Option<PanelCmd> {
    let mut command = None;
    ui.horizontal(|ui| {
        let button = ui.add(egui::Button::new(egui::RichText::new("FSOT").size(16.0).strong()).selected(*desk_open));
        if button.clicked() {
            *desk_open = !*desk_open;
        }
        button.on_hover_text("Measured fields. FSOT is not scoring.");
        let editor = ui.add(
            egui::TextEdit::singleline(&mut panel.query)
                .hint_text("City, town, or postal code")
                .desired_width(ui.available_width()),
        );
        if editor.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            panel.submit = true;
        }
    });
    let query = panel.query.trim().to_string();
    if query.chars().count() < 2 {
        panel.submit = false;
        if panel.saved.is_empty() {
            ui.label(egui::RichText::new("Search a city, town, or postal code. Enter opens the first match.").size(12.0).color(muted));
        } else {
            ui.label(egui::RichText::new("Saved places").size(11.0).color(muted));
            ui.horizontal_wrapped(|ui| {
                for (index, place) in panel.saved.iter().enumerate() {
                    let label = if place.home { format!("Home · {}", place.name) } else { place.name.clone() };
                    if ui.button(label).clicked() {
                        command = Some(PanelCmd::OpenSaved(index));
                    }
                }
            });
        }
        return command;
    }
    let shown = matching_places(feeds, &query, geocode);
    match geocode {
        None => {
            ui.label(egui::RichText::new("Searching towns and postal codes…").size(12.0).color(muted));
        }
        Some(Err(_)) => {
            ui.label(egui::RichText::new("Open-Meteo place search did not load. Showing loaded city names.").size(12.0).color(muted));
        }
        Some(Ok(hits)) if hits.is_empty() && shown.is_empty() => {
            ui.label(egui::RichText::new("No town or postal code matched.").size(12.0).color(muted));
        }
        Some(Ok(hits)) if hits.is_empty() => {
            ui.label(egui::RichText::new("No town or postal code matched. Showing loaded city names.").size(12.0).color(muted));
        }
        Some(Ok(_)) => {}
    }
    for place in &shown {
        if ui.add(egui::Button::new(&place.name).wrap_mode(egui::TextWrapMode::Wrap)).clicked() {
            command = Some(PanelCmd::Choose(place.clone()));
        }
    }
    if panel.submit && command.is_none() {
        let ready = !matches!(geocode, None);
        if ready {
            panel.submit = false;
            if let Some(place) = shown.first() {
                command = Some(PanelCmd::Choose(place.clone()));
            }
        }
    }
    command
}

fn place_card(
    ui: &mut egui::Ui,
    feeds: &FeedState,
    panel: &mut PanelState,
    forecast: &Option<Result<feeds::PlaceForecast, String>>,
    ink: egui::Color32,
    muted: egui::Color32,
) -> Option<PanelCmd> {
    let mut command = None;
    let Some(spot) = panel.spot.clone() else { return None };
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(egui::RichText::new("Local weather").size(16.0).strong().color(ink));
        ui.label(egui::RichText::new(&spot.name).size(18.0).strong().color(ink));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Save place").clicked() {
                command = Some(PanelCmd::Save);
            }
            if ui.button("Make home").clicked() {
                command = Some(PanelCmd::MakeHome);
            }
        });
        unit_row(ui, &mut panel.units);
        ui.label(egui::RichText::new("This card only. The reading keeps each agency's units.").size(11.0).color(muted));
        ui.add_space(4.0);
        match forecast {
            None => {
                ui.label(egui::RichText::new("Open-Meteo hourly forecast for this place is loading.").size(12.0).color(muted));
            }
            Some(Err(_)) => {
                ui.label(egui::RichText::new("Open-Meteo hourly forecast did not load for this place.").size(12.0).color(muted));
            }
            Some(Ok(cast)) => {
                for line in feeds::format_place_forecast(cast, panel.units) {
                    ui.label(egui::RichText::new(line).size(12.0).color(ink));
                }
            }
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Nearest loaded samples, with distance.").size(11.0).color(muted));
        for line in feeds::describe_spot(spot.lat, spot.lon, &feeds.wind, &feeds.quakes, &feeds.bulletin, panel.units) {
            ui.label(egui::RichText::new(line).size(12.0).color(ink));
            ui.add_space(2.0);
        }
    });
    command
}

fn unit_row(ui: &mut egui::Ui, units: &mut feeds::ShowUnits) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut units.fahrenheit, false, "°C");
        ui.selectable_value(&mut units.fahrenheit, true, "°F");
        ui.selectable_value(&mut units.mph, false, "m/s");
        ui.selectable_value(&mut units.mph, true, "mph");
        ui.selectable_value(&mut units.inches, false, "mm");
        ui.selectable_value(&mut units.inches, true, "in");
    });
}

fn watch_block(ui: &mut egui::Ui, feeds: &FeedState, ink: egui::Color32, muted: egui::Color32) -> Option<PanelCmd> {
    let mut command = None;
    let cards = feeds::watch_cards(&feeds.quakes, &feeds.bulletin);
    if cards.is_empty() {
        ui.label(egui::RichText::new("Agency watch: no warning, watch, storm, or tsunami bulletin is loaded.").size(12.0).color(ink));
    }
    for card in cards {
        if let (Some(lat), Some(lon)) = (card.lat, card.lon) {
            let name = if card.place.is_empty() { card.title.clone() } else { card.place.clone() };
            if ui.add(egui::Button::new(&card.title).wrap_mode(egui::TextWrapMode::Wrap)).clicked() {
                command = Some(PanelCmd::Choose(Spot { name, lat, lon }));
            }
        } else {
            ui.label(egui::RichText::new(&card.title).size(12.0).color(ink));
        }
        if !card.detail.is_empty() {
            ui.label(egui::RichText::new(&card.detail).size(11.0).color(muted));
        }
        ui.add_space(3.0);
    }
    command
}

fn monitor_column(
    ui: &mut egui::Ui,
    layers: &mut Layers,
    feeds: &FeedState,
    panel: &mut PanelState,
    readout: &Readout,
    ink: egui::Color32,
    muted: egui::Color32,
) -> Option<PanelCmd> {
    let mut command = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(egui::RichText::new("Monitor").size(16.0).strong().color(ink));
        section(ui, "WATCH");
        if let Some(next) = watch_block(ui, feeds, ink, muted) {
            command = Some(next);
        }
        section(ui, "LAYERS");
        ui.label(egui::RichText::new("NASA Blue Marble").size(11.0).color(muted));
        check(ui, &mut layers.labels, "Labels", "continents, oceans, seas");
        check(ui, &mut layers.graticule, "Graticule", "15° lines, 5° when closer");
        check(ui, &mut layers.sun, "Sun lighting", "real UTC terminator");
        check(ui, &mut layers.city_lights, "City lights", "static NASA night image");
        check(ui, &mut layers.atmosphere, "Atmosphere", "limb and halo");
        check(ui, &mut layers.stars, "Stars", "procedural background");
        ui.checkbox(&mut layers.reading, "Reading along the bottom");
        section(ui, "WORLD");
        toggle(ui, &mut layers.cities, "City names", &feeds.cities_status);
        toggle(ui, &mut layers.elevation, "Elevation", &feeds.elevation_status);
        toggle(ui, &mut layers.weather, "Weather", &feeds.weather_status);
        toggle(ui, &mut layers.quakes, "Earthquakes", &feeds.quake_status);
        toggle(ui, &mut layers.faults, "Faults", &feeds.fault_status);
        toggle(ui, &mut layers.volcanoes, "Volcanoes", &feeds.volcano_status);
        toggle(ui, &mut layers.ocean, "Ocean", &feeds.ocean_status);
        section(ui, "SCALE");
        let top = feeds::speed_text(18.0, panel.units);
        ui.label(egui::RichText::new(format!("Wind arrows point where the wind is going. Longer and brighter is stronger. {top} is near the long end.")).size(11.0).color(muted));
        ui.label(egui::RichText::new("Quake discs grow with magnitude. Gold is under M4, orange is M4, red is M5, white is M6.").size(11.0).color(muted));
        section(ui, "VIEW");
        row(ui, "Cursor", &readout.cursor);
        row(ui, "Altitude", &readout.altitude);
        row(ui, "Heading", &readout.heading);
        row(ui, "Tilt", &readout.tilt);
        row(ui, "UTC", &readout.utc);
        row(ui, "Sun", &readout.sun);
        row(ui, "Frame", &format!("{} fps", readout.fps));
        section(ui, "CONTROLS");
        for line in [
            "FSOT opens the measured-field desk",
            "Type a place and press Enter",
            "Click a result to fly there",
            "Click the globe for weather there",
            "Drag to pan · scroll to zoom",
            "Right-drag to tilt · double-click zooms",
            "Arrows pan · + and − zoom",
            "R resets the view · Esc closes",
        ] {
            ui.label(egui::RichText::new(line).size(12.0).color(muted));
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Places: Open-Meteo geocoding, GeoNames. Imagery: NASA Blue Marble, public domain. City lights: NASA Earth at night. Cities: Natural Earth names. Faults: Bird 2002. Volcanoes: Smithsonian GVP, USGS alerts, Smithsonian/USGS weekly report. Quakes: USGS. Solar wind, Kp, X-ray flares, plasma, protons, electrons, magnetometer, alerts, F10.7, solar regions, sunspots, Dst, and NOAA scales: NOAA SWPC. CMEs: NASA DONKI, SIDC CACTus. ONI, AO, and NAO: CPC. MJO: BOM RMM. Sea ice: NSIDC. CO2: NOAA GML Mauna Loa. Active fire detections: NASA FIRMS. Storms: NHC, JTWC. Tsunami bulletins: NWS. Volcanic ash advisories: NWS relay of VAAC texts. Sea level: UNESCO IOC. Weather, sea, the place ensemble, and place air quality: Open-Meteo.").size(10.0).color(egui::Color32::from_rgb(110, 122, 136)));
    });
    command
}

fn reading_strip(ui: &mut egui::Ui, feeds: &FeedState, ink: egui::Color32) {
    ui.label(egui::RichText::new("Reading").size(12.0).strong().color(ink));
    egui::ScrollArea::vertical().max_height(112.0).show(ui, |ui| {
        for line in &feeds.reading {
            ui.label(egui::RichText::new(line).size(12.0).color(ink));
            ui.add_space(2.0);
        }
    });
}

fn hud(
    ui: &mut egui::Ui,
    layers: &mut Layers,
    feeds: &FeedState,
    panel: &mut PanelState,
    forecast: &Option<Result<feeds::PlaceForecast, String>>,
    ensemble: &Option<Result<feeds::PlaceEnsemble, String>>,
    air: &Option<Result<feeds::PlaceAir, String>>,
    geocode: &Option<Result<Vec<feeds::GeoHit>, String>>,
    readout: &Readout,
    labels: &[ScreenLabel],
    feed_labels: &[DynLabel],
    ppp: f32,
    desk_open: &mut bool,
    chrome: &mut Chrome,
) -> Option<PanelCmd> {
    let muted = egui::Color32::from_rgb(148, 164, 180);
    let ink = egui::Color32::from_rgb(232, 238, 244);
    let mut command = None;

    let search = egui::Panel::top("search").show_separator_line(false).frame(panel_frame()).show(ui, |ui| {
        search_bar(ui, feeds, panel, geocode, desk_open, muted)
    });
    chrome.top = search.response.rect;
    take_cmd(&mut command, search.inner);

    if layers.reading {
        let reading = egui::Panel::bottom("reading")
            .resizable(false)
            .exact_size(156.0)
            .show_separator_line(false)
            .frame(panel_frame())
            .show(ui, |ui| reading_strip(ui, feeds, ink));
        chrome.bottom = reading.response.rect;
    }

    let monitor = egui::Panel::left("monitor")
        .resizable(false)
        .exact_size(276.0)
        .show_separator_line(false)
        .frame(panel_frame())
        .show(ui, |ui| monitor_column(ui, layers, feeds, panel, readout, ink, muted));
    chrome.left = monitor.response.rect;
    take_cmd(&mut command, monitor.inner);

    if panel.spot.is_some() {
        let place = egui::Panel::right("place")
            .resizable(false)
            .exact_size(328.0)
            .show_separator_line(false)
            .frame(panel_frame())
            .show(ui, |ui| place_card(ui, feeds, panel, forecast, ink, muted));
        chrome.right = place.response.rect;
        take_cmd(&mut command, place.inner);
    }

    let globe = ui.available_rect_before_wrap();
    let painter = ui.painter_at(globe);
    if layers.labels {
        for label in labels {
            paint_label(&painter, label, ppp);
        }
    }
    for label in feed_labels {
        paint_dyn(&painter, label, ppp);
    }
    painter.circle_stroke(
        globe.center(),
        4.5,
        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 70)),
    );
    if *desk_open {
        let frame = desk_frame(ui.style());
        let title = desk_frame(ui.style());
        let shown = egui::Window::new("FSOT data desk")
            .open(desk_open)
            .collapsible(false)
            .resizable(true)
            .default_pos(egui::pos2(300.0, 128.0))
            .default_size(egui::vec2(460.0, 520.0))
            .frame(frame)
            .title_frame(title)
            .show(ui.ctx(), |ui| science_desk(ui, feeds, panel.spot.is_some(), ensemble, air, ink));
        if let Some(shown) = shown {
            chrome.desk = shown.response.rect;
        }
    }
    command
}

fn desk_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style).fill(egui::Color32::from_rgba_unmultiplied(8, 14, 24, 238))
}

fn science_desk(
    ui: &mut egui::Ui,
    feeds: &FeedState,
    place_selected: bool,
    ensemble: &Option<Result<feeds::PlaceEnsemble, String>>,
    air: &Option<Result<feeds::PlaceAir, String>>,
    ink: egui::Color32,
) {
    ui.label(egui::RichText::new(feeds::desk_banner()).size(13.0).strong().color(ink));
    ui.add_space(4.0);
    egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
        for section in feeds::desk_sections(&feeds.quakes, &feeds.bulletin, place_selected, ensemble, air) {
            ui.label(egui::RichText::new(section.title).size(11.0).strong().color(egui::Color32::from_rgb(132, 186, 214)));
            for line in section.lines {
                ui.label(egui::RichText::new(line).size(12.0).color(ink));
                ui.add_space(2.0);
            }
            ui.add_space(6.0);
        }
    });
}

fn paint_dyn(painter: &egui::Painter, label: &DynLabel, ppp: f32) {
    let pos = egui::pos2(label.x / ppp, label.y / ppp);
    let alpha = (label.alpha * 255.0).clamp(0.0, 255.0) as u8;
    let color = egui::Color32::from_rgba_unmultiplied(label.rgb[0], label.rgb[1], label.rgb[2], alpha);
    let shadow = egui::Color32::from_rgba_unmultiplied(0, 0, 0, (alpha as f32 * 0.72) as u8);
    let font = egui::FontId::proportional(label.size);
    painter.text(pos + egui::vec2(1.0, 1.2), egui::Align2::CENTER_BOTTOM, &label.text, font.clone(), shadow);
    painter.text(pos, egui::Align2::CENTER_BOTTOM, &label.text, font, color);
}

fn paint_label(painter: &egui::Painter, label: &ScreenLabel, ppp: f32) {
    let pos = egui::pos2(label.x / ppp, label.y / ppp);
    let font = egui::FontId::proportional(label.size);
    let alpha = (label.alpha * 255.0).clamp(0.0, 255.0) as u8;
    let color = match label.kind {
        LabelKind::Continent => egui::Color32::from_rgba_unmultiplied(246, 248, 252, alpha),
        LabelKind::Ocean => egui::Color32::from_rgba_unmultiplied(168, 214, 232, (alpha as f32 * 0.92) as u8),
        LabelKind::Sea => egui::Color32::from_rgba_unmultiplied(186, 224, 214, (alpha as f32 * 0.9) as u8),
    };
    let shadow = egui::Color32::from_rgba_unmultiplied(0, 0, 0, (alpha as f32 * 0.72) as u8);
    painter.text(pos + egui::vec2(1.0, 1.2), egui::Align2::CENTER_CENTER, label.name, font.clone(), shadow);
    painter.text(pos, egui::Align2::CENTER_CENTER, label.name, font, color);
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(title).size(11.0).strong().color(egui::Color32::from_rgb(132, 186, 214)));
    ui.add_space(2.0);
}

fn row(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(key).size(12.5).color(egui::Color32::from_rgb(142, 158, 174)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).size(12.5).color(egui::Color32::from_rgb(230, 236, 242)));
        });
    });
}

fn check(ui: &mut egui::Ui, value: &mut bool, name: &str, detail: &str) {
    ui.checkbox(value, name).on_hover_text(detail);
}

fn toggle(ui: &mut egui::Ui, value: &mut bool, name: &str, detail: &str) {
    ui.checkbox(value, name);
    ui.label(egui::RichText::new(detail).size(11.0).color(egui::Color32::from_rgb(130, 146, 162)));
    ui.add_space(2.0);
}

fn alert(message: &str) {
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        let wide = |text: &str| OsStr::new(text).encode_wide().chain(std::iter::once(0)).collect::<Vec<_>>();
        let body = wide(message);
        let title = wide("FSOT Global Monitor");
        unsafe {
            MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), 0x0000_0010);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = message;
    }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(hwnd: *mut core::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
}

fn install_ui_font(ctx: &egui::Context) {
    // egui 0.37's bundled-font crate is not published, so the panel uses a
    // local Windows face. Labels stay blank if none of these files exist.
    let bytes = [
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\calibri.ttf",
    ]
    .iter()
    .find_map(|path| std::fs::read(path).ok());
    let Some(bytes) = bytes else {
        log::warn!("no UI font found; labels will be blank");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "ui".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "ui".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push("ui".to_owned());
    ctx.set_fonts(fonts);
}

fn style_ui(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::TRANSPARENT;
    visuals.window_fill = egui::Color32::TRANSPARENT;
    visuals.extreme_bg_color = egui::Color32::from_rgb(16, 24, 36);
    visuals.override_text_color = Some(egui::Color32::from_rgb(226, 232, 238));
    ctx.set_visuals(visuals);
}

fn format_km(km: f32) -> String {
    if km >= 100.0 {
        format!("{:.0} km", km)
    } else {
        format!("{:.1} km", km)
    }
}

#[allow(dead_code)]
fn frame_has_detail(image: &RgbaImage) -> bool {
    let w = image.width();
    let h = image.height();
    let sx = (w / 24).max(1);
    let sy = (h / 24).max(1);
    let mut min_l = 255u8;
    let mut max_l = 0u8;
    let mut y = 0;
    while y < h {
        let mut x = 0;
        while x < w {
            let p = image.get_pixel(x, y).0;
            let l = p[0].max(p[1]).max(p[2]);
            min_l = min_l.min(l);
            max_l = max_l.max(l);
            x += sx;
        }
        y += sy;
    }
    max_l.saturating_sub(min_l) > 16
}

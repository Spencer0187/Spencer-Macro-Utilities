use crate::{DisplayError, PixelColor, ScreenBounds};
use ashpd::desktop::{
    CreateSessionOptions, PersistMode,
    remote_desktop::{
        DeviceType, NotifyPointerMotionAbsoluteOptions, RemoteDesktop, SelectDevicesOptions,
        StartOptions,
    },
    screencast::{
        CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
        StartCastOptions,
    },
};
use pipewire as pw;
use pw::{properties::properties, spa};
use std::fmt;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const PORTAL_START_TIMEOUT: Duration = Duration::from_secs(120);
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const POINTER_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PortalMode {
    ScreenCast,
    RemoteDesktop,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum PortalPhase {
    #[default]
    Inactive,
    Starting,
    Active,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivationKind {
    ScreenCast,
    RemoteDesktop,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ActivationPhase {
    #[default]
    Idle,
    AwaitingUser,
    Starting,
}

#[derive(Debug, Default)]
struct ActivationState {
    phase: ActivationPhase,
    kind: Option<ActivationKind>,
    generation: u64,
    completed_generation: u64,
    succeeded: bool,
    error: String,
}

#[derive(Debug, Default)]
struct ActivationControl {
    state: Mutex<ActivationState>,
    changed: Condvar,
}

#[derive(Debug)]
struct PortalState {
    phase: PortalPhase,
    mode: Option<PortalMode>,
    frame: Vec<u8>,
    frame_width: i32,
    frame_height: i32,
    logical_width: i32,
    logical_height: i32,
    stream_node: u32,
    status: String,
}

impl Default for PortalState {
    fn default() -> Self {
        Self {
            phase: PortalPhase::Inactive,
            mode: None,
            frame: Vec::new(),
            frame_width: 0,
            frame_height: 0,
            logical_width: 0,
            logical_height: 0,
            stream_node: 0,
            status: "Wayland ScreenCast is inactive.".to_owned(),
        }
    }
}

#[derive(Debug)]
struct SharedState {
    state: Mutex<PortalState>,
    changed: Condvar,
}

impl Default for SharedState {
    fn default() -> Self {
        Self { state: Mutex::new(PortalState::default()), changed: Condvar::new() }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[derive(Debug)]
enum ControlMessage {
    Stop,
    MovePointer { x: f64, y: f64, response: mpsc::Sender<Result<(), String>> },
}

/// User-approved native Wayland ScreenCast/RemoteDesktop session.
///
/// Portal and `PipeWire` objects stay on one owned worker thread. Macro/script callers only exchange
/// immutable frame snapshots and small control messages with that worker.
pub struct WaylandPortalCapture {
    shared: Arc<SharedState>,
    control: Mutex<Option<pw::channel::Sender<ControlMessage>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    activation: ActivationControl,
}

impl fmt::Debug for WaylandPortalCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.shared.state);
        formatter
            .debug_struct("WaylandPortalCapture")
            .field("active", &(state.phase == PortalPhase::Active))
            .field("remote_desktop", &(state.mode == Some(PortalMode::RemoteDesktop)))
            .field("frame_width", &state.frame_width)
            .field("frame_height", &state.frame_height)
            .field("status", &state.status)
            .finish_non_exhaustive()
    }
}

impl Default for WaylandPortalCapture {
    fn default() -> Self {
        Self {
            shared: Arc::new(SharedState::default()),
            control: Mutex::new(None),
            worker: Mutex::new(None),
            activation: ActivationControl::default(),
        }
    }
}

impl WaylandPortalCapture {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn is_supported(&self) -> bool {
        true
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        lock(&self.shared.state).phase == PortalPhase::Active
    }

    #[must_use]
    pub fn has_remote_desktop_control(&self) -> bool {
        let state = lock(&self.shared.state);
        state.phase == PortalPhase::Active && state.mode == Some(PortalMode::RemoteDesktop)
    }

    #[must_use]
    pub fn status(&self) -> String {
        lock(&self.shared.state).status.clone()
    }

    /// Wait for the application/UI thread to approve `ScreenCast` access for a script.
    ///
    /// # Errors
    ///
    /// Returns user-decline, cancellation, or portal-start failures.
    pub fn request_activation_for_script(
        &self,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), DisplayError> {
        self.request_activation(ActivationKind::ScreenCast, cancelled)
    }

    /// Wait for the application/UI thread to approve `RemoteDesktop` pointer access for a script.
    ///
    /// # Errors
    ///
    /// Returns user-decline, cancellation, or portal-start failures.
    pub fn request_remote_desktop_activation_for_script(
        &self,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), DisplayError> {
        self.request_activation(ActivationKind::RemoteDesktop, cancelled)
    }

    #[must_use]
    pub fn has_pending_activation_request(&self) -> bool {
        let state = lock(&self.activation.state);
        state.phase == ActivationPhase::AwaitingUser
            && state.kind == Some(ActivationKind::ScreenCast)
    }

    #[must_use]
    pub fn has_pending_remote_desktop_activation_request(&self) -> bool {
        let state = lock(&self.activation.state);
        state.phase == ActivationPhase::AwaitingUser
            && state.kind == Some(ActivationKind::RemoteDesktop)
    }

    pub fn approve_activation_request(&self) {
        self.approve_activation(ActivationKind::ScreenCast);
    }

    pub fn approve_remote_desktop_activation_request(&self) {
        self.approve_activation(ActivationKind::RemoteDesktop);
    }

    pub fn decline_activation_request(&self) {
        self.decline_activation(ActivationKind::ScreenCast);
    }

    pub fn decline_remote_desktop_activation_request(&self) {
        self.decline_activation(ActivationKind::RemoteDesktop);
    }

    #[allow(clippy::significant_drop_tightening)] // The guard is the Condvar wait token.
    fn request_activation(
        &self,
        kind: ActivationKind,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), DisplayError> {
        let already_active = match kind {
            ActivationKind::ScreenCast => self.is_active(),
            ActivationKind::RemoteDesktop => self.has_remote_desktop_control(),
        };
        if already_active {
            return Ok(());
        }

        let mut state = lock(&self.activation.state);
        let now_active = match kind {
            ActivationKind::ScreenCast => self.is_active(),
            ActivationKind::RemoteDesktop => self.has_remote_desktop_control(),
        };
        if now_active {
            return Ok(());
        }
        if state.phase == ActivationPhase::Idle {
            state.phase = ActivationPhase::AwaitingUser;
            state.kind = Some(kind);
            state.generation = state.generation.wrapping_add(1);
        } else if state.kind != Some(kind) {
            return Err(DisplayError::new(
                "another Wayland portal permission request is already awaiting a decision",
            ));
        }
        let generation = state.generation;
        while state.completed_generation < generation {
            if cancelled() {
                return Err(DisplayError::new(
                    "script was stopped while waiting for Wayland portal permission",
                ));
            }
            let waited = self.activation.changed.wait_timeout(state, Duration::from_millis(100));
            state = match waited {
                Ok((state, _)) => state,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        if state.succeeded {
            return Ok(());
        }
        let message = if state.error.is_empty() {
            "Wayland portal permission was declined."
        } else {
            state.error.as_str()
        };
        Err(DisplayError::new(message))
    }

    fn approve_activation(&self, kind: ActivationKind) {
        let generation = {
            let mut state = lock(&self.activation.state);
            if state.phase != ActivationPhase::AwaitingUser || state.kind != Some(kind) {
                return;
            }
            state.phase = ActivationPhase::Starting;
            state.generation
        };

        let result = match kind {
            ActivationKind::ScreenCast => self.start(),
            ActivationKind::RemoteDesktop => self.start_remote_desktop(),
        };
        {
            let mut state = lock(&self.activation.state);
            state.phase = ActivationPhase::Idle;
            state.kind = None;
            state.completed_generation = generation;
            state.succeeded = result.is_ok();
            state.error = result.err().map_or_else(String::new, |error| error.to_string());
        }
        self.activation.changed.notify_all();
    }

    fn decline_activation(&self, kind: ActivationKind) {
        {
            let mut state = lock(&self.activation.state);
            if state.phase != ActivationPhase::AwaitingUser || state.kind != Some(kind) {
                return;
            }
            state.phase = ActivationPhase::Idle;
            state.kind = None;
            state.completed_generation = state.generation;
            state.succeeded = false;
            state.error = match kind {
                ActivationKind::ScreenCast => "Wayland screen capture was declined. getPixelColor and getPixelRect require selecting a monitor.".to_owned(),
                ActivationKind::RemoteDesktop => "Wayland absolute mouse control was declined. moveMouseAbs requires selecting a monitor and allowing pointer control.".to_owned(),
            };
        }
        self.activation.changed.notify_all();
    }

    /// Open the desktop portal monitor picker and start `PipeWire` capture.
    ///
    /// # Errors
    ///
    /// Returns portal, `PipeWire`, user-decline, or first-frame timeout failures.
    pub fn start(&self) -> Result<(), DisplayError> {
        self.start_mode(PortalMode::ScreenCast)
    }

    /// Replace the active capture with a combined `RemoteDesktop` + `ScreenCast` session.
    ///
    /// # Errors
    ///
    /// Returns portal, pointer-grant, `PipeWire`, or first-frame timeout failures.
    pub fn start_remote_desktop(&self) -> Result<(), DisplayError> {
        self.start_mode(PortalMode::RemoteDesktop)
    }

    #[allow(clippy::significant_drop_tightening)] // Condvar waits intentionally retain the guarded state.
    fn start_mode(&self, mode: PortalMode) -> Result<(), DisplayError> {
        self.stop();
        {
            let mut state = lock(&self.shared.state);
            *state = PortalState::default();
            state.phase = PortalPhase::Starting;
            state.mode = Some(mode);
            state.status = match mode {
                PortalMode::ScreenCast => {
                    "Requesting monitor access from the desktop portal...".to_owned()
                }
                PortalMode::RemoteDesktop => {
                    "Requesting Wayland absolute pointer and monitor access from the desktop portal..."
                        .to_owned()
                }
            };
        }

        let (sender, receiver) = pw::channel::channel();
        *lock(&self.control) = Some(sender);
        let shared = Arc::clone(&self.shared);
        let worker = std::thread::spawn(move || match mode {
            PortalMode::ScreenCast => run_screencast_worker(&shared, receiver),
            PortalMode::RemoteDesktop => run_remote_desktop_worker(&shared, receiver),
        });
        *lock(&self.worker) = Some(worker);

        let mut state = lock(&self.shared.state);
        let waited = self.shared.changed.wait_timeout_while(state, PORTAL_START_TIMEOUT, |state| {
            state.phase == PortalPhase::Starting
        });
        let (next_state, timeout) = match waited {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        };
        state = next_state;
        if timeout.timed_out() && state.phase == PortalPhase::Starting {
            drop(state);
            self.stop();
            return Err(DisplayError::new(match mode {
                PortalMode::ScreenCast => {
                    "Timed out waiting for Wayland ScreenCast portal permission."
                }
                PortalMode::RemoteDesktop => {
                    "Timed out waiting for Wayland RemoteDesktop portal permission."
                }
            }));
        }
        if state.phase == PortalPhase::Failed {
            let message = state.status.clone();
            drop(state);
            self.join_finished_worker();
            return Err(DisplayError::new(message));
        }
        drop(state);

        let state = lock(&self.shared.state);
        let waited = self.shared.changed.wait_timeout_while(state, FIRST_FRAME_TIMEOUT, |state| {
            state.phase == PortalPhase::Active && state.frame.is_empty()
        });
        let (state, timeout) = match waited {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        };
        if timeout.timed_out() && state.frame.is_empty() && state.phase != PortalPhase::Failed {
            drop(state);
            self.stop();
            return Err(DisplayError::new(match mode {
                PortalMode::ScreenCast => {
                    "Wayland ScreenCast did not deliver a frame within 5 seconds."
                }
                PortalMode::RemoteDesktop => {
                    "Wayland RemoteDesktop did not deliver a frame within 5 seconds."
                }
            }));
        }
        if state.phase != PortalPhase::Active || state.frame.is_empty() {
            let message = if state.status.is_empty() {
                "Wayland portal session could not be started.".to_owned()
            } else {
                state.status.clone()
            };
            drop(state);
            self.stop();
            return Err(DisplayError::new(message));
        }
        drop(state);
        Ok(())
    }

    pub fn stop(&self) {
        let sender = lock(&self.control).take();
        if let Some(sender) = sender {
            let _ = sender.send(ControlMessage::Stop);
        }
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
        let mut state = lock(&self.shared.state);
        *state = PortalState::default();
        drop(state);
        self.shared.changed.notify_all();
    }

    fn join_finished_worker(&self) {
        let worker = {
            let mut slot = lock(&self.worker);
            if slot.as_ref().is_some_and(JoinHandle::is_finished) { slot.take() } else { None }
        };
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }

    #[must_use]
    pub fn selected_monitor_bounds(&self) -> Option<ScreenBounds> {
        let state = lock(&self.shared.state);
        (state.frame_width > 0 && state.frame_height > 0).then_some(ScreenBounds {
            x: 0,
            y: 0,
            width: state.frame_width,
            height: state.frame_height,
        })
    }

    /// Sample one pixel in the selected monitor's captured coordinate space.
    ///
    /// # Errors
    ///
    /// Returns inactive/no-frame/out-of-bounds errors matching the C++ portal service.
    pub fn sample(&self, x: i32, y: i32) -> Result<PixelColor, DisplayError> {
        let state = lock(&self.shared.state);
        if state.phase != PortalPhase::Active {
            return Err(DisplayError::new(
                "Wayland screen reads require a ScreenCast session. Open Settings and choose Enable Wayland Screen Capture.",
            ));
        }
        if state.frame_width <= 0 || state.frame_height <= 0 || state.frame.is_empty() {
            return Err(DisplayError::new(
                "Wayland ScreenCast is active but has not delivered a frame yet.",
            ));
        }
        if x < 0 || y < 0 || x >= state.frame_width || y >= state.frame_height {
            return Err(DisplayError::new(
                "requested pixel is outside the captured Wayland monitor.",
            ));
        }
        let x = usize::try_from(x).map_err(|_| {
            DisplayError::new("requested pixel is outside the captured Wayland monitor.")
        })?;
        let y = usize::try_from(y).map_err(|_| {
            DisplayError::new("requested pixel is outside the captured Wayland monitor.")
        })?;
        let width = usize::try_from(state.frame_width)
            .map_err(|_| DisplayError::new("Wayland ScreenCast frame dimensions are invalid."))?;
        let offset = y
            .checked_mul(width)
            .and_then(|value| value.checked_add(x))
            .and_then(|value| value.checked_mul(3))
            .ok_or_else(|| DisplayError::new("Wayland ScreenCast frame dimensions are invalid."))?;
        let Some(pixel) = state.frame.get(offset..offset + 3) else {
            return Err(DisplayError::new("Wayland ScreenCast frame is incomplete."));
        };
        let color = PixelColor { r: pixel[0], g: pixel[1], b: pixel[2] };
        drop(state);
        Ok(color)
    }

    /// Move the Wayland pointer using the active `RemoteDesktop` session.
    ///
    /// # Errors
    ///
    /// Returns inactive/out-of-bounds/portal communication failures.
    pub fn move_pointer_absolute(&self, x: i32, y: i32) -> Result<(), DisplayError> {
        let (logical_x, logical_y) = {
            let state = lock(&self.shared.state);
            if state.phase != PortalPhase::Active
                || state.mode != Some(PortalMode::RemoteDesktop)
                || state.stream_node == 0
                || state.frame_width <= 0
                || state.frame_height <= 0
            {
                return Err(DisplayError::new(
                    "absolute mouse positioning on Wayland requires enabling Wayland Absolute Mouse Control and selecting a monitor.",
                ));
            }
            if x < 0 || y < 0 || x >= state.frame_width || y >= state.frame_height {
                return Err(DisplayError::new(
                    "requested pointer position is outside the selected Wayland monitor.",
                ));
            }
            let logical_width =
                if state.logical_width > 0 { state.logical_width } else { state.frame_width };
            let logical_height =
                if state.logical_height > 0 { state.logical_height } else { state.frame_height };
            (
                map_pointer_coordinate(x, state.frame_width, logical_width),
                map_pointer_coordinate(y, state.frame_height, logical_height),
            )
        };

        let sender = lock(&self.control)
            .clone()
            .ok_or_else(|| DisplayError::new("Wayland RemoteDesktop session is not running."))?;
        let (response_tx, response_rx) = mpsc::channel();
        sender
            .send(ControlMessage::MovePointer { x: logical_x, y: logical_y, response: response_tx })
            .map_err(|_| DisplayError::new("Wayland RemoteDesktop session is not running."))?;
        match response_rx.recv_timeout(POINTER_COMMAND_TIMEOUT) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(DisplayError::new(error)),
            Err(_) => {
                Err(DisplayError::new("Wayland RemoteDesktop pointer-motion request timed out."))
            }
        }
    }
}

impl Drop for WaylandPortalCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn map_pointer_coordinate(value: i32, frame_extent: i32, logical_extent: i32) -> f64 {
    if frame_extent <= 1 || logical_extent <= 1 {
        0.0
    } else {
        f64::from(value) * f64::from(logical_extent - 1) / f64::from(frame_extent - 1)
    }
}

fn set_failure(shared: &SharedState, message: impl Into<String>) {
    let mut state = lock(&shared.state);
    state.phase = PortalPhase::Failed;
    state.status = message.into();
    drop(state);
    shared.changed.notify_all();
}

fn mark_portal_ready(
    shared: &SharedState,
    remote_desktop: bool,
    stream_node: u32,
    logical_width: i32,
    logical_height: i32,
) {
    let mut state = lock(&shared.state);
    state.phase = PortalPhase::Active;
    state.mode =
        Some(if remote_desktop { PortalMode::RemoteDesktop } else { PortalMode::ScreenCast });
    state.stream_node = stream_node;
    state.logical_width = logical_width;
    state.logical_height = logical_height;
    state.status = if remote_desktop {
        "Wayland RemoteDesktop is active. Pixel reads and moveMouseAbs target the monitor selected in the portal."
            .to_owned()
    } else {
        "Wayland ScreenCast is active. Pixel APIs target the monitor selected in the portal."
            .to_owned()
    };
    drop(state);
    shared.changed.notify_all();
}

fn mark_stopped(shared: &SharedState) {
    let mut state = lock(&shared.state);
    let failed = state.phase == PortalPhase::Failed;
    state.phase = if failed { PortalPhase::Failed } else { PortalPhase::Inactive };
    state.mode = None;
    state.stream_node = 0;
    if !failed {
        "Wayland ScreenCast is inactive.".clone_into(&mut state.status);
    }
    drop(state);
    shared.changed.notify_all();
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Could not start the Wayland portal runtime: {error}"))
}

fn run_screencast_worker(
    shared: &Arc<SharedState>,
    receiver: pw::channel::Receiver<ControlMessage>,
) {
    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            set_failure(shared, error);
            return;
        }
    };
    let portal = runtime.block_on(async {
        let proxy = Screencast::new().await?;
        let session = proxy.create_session(CreateSessionOptions::default()).await?;
        proxy
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::Hidden)
                    .set_sources(ashpd::enumflags2::BitFlags::from_flag(SourceType::Monitor))
                    .set_multiple(false)
                    .set_restore_token(None)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await?
            .response()?;
        let response =
            proxy.start(&session, None, StartCastOptions::default()).await?.response()?;
        let stream = response.streams().first().cloned().ok_or(ashpd::Error::NoResponse)?;
        let fd =
            proxy.open_pipe_wire_remote(&session, OpenPipeWireRemoteOptions::default()).await?;
        Ok::<_, ashpd::Error>((session, stream, fd))
    });
    let (session, stream, fd) = match portal {
        Ok(value) => value,
        Err(error) => {
            set_failure(shared, format!("Could not start Wayland ScreenCast: {error}"));
            return;
        }
    };
    let node_id = stream.pipe_wire_node_id();
    let logical = stream.size().unwrap_or((0, 0));
    mark_portal_ready(shared, false, node_id, logical.0, logical.1);
    if let Err(error) = run_pipewire_loop(node_id, fd, shared, receiver, None) {
        set_failure(shared, error);
    }
    let _ = runtime.block_on(session.close());
    mark_stopped(shared);
}

fn run_remote_desktop_worker(
    shared: &Arc<SharedState>,
    receiver: pw::channel::Receiver<ControlMessage>,
) {
    let runtime = match runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            set_failure(shared, error);
            return;
        }
    };
    let portal = runtime.block_on(async {
        let remote = RemoteDesktop::new().await?;
        let screencast = Screencast::new().await?;
        let session = remote.create_session(CreateSessionOptions::default()).await?;
        remote
            .select_devices(
                &session,
                SelectDevicesOptions::default()
                    .set_devices(ashpd::enumflags2::BitFlags::from_flag(DeviceType::Pointer))
                    .set_persist_mode(PersistMode::DoNot)
                    .set_restore_token(None),
            )
            .await?
            .response()?;
        screencast
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::Hidden)
                    .set_sources(ashpd::enumflags2::BitFlags::from_flag(SourceType::Monitor))
                    .set_multiple(false)
                    .set_restore_token(None)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await?
            .response()?;
        let response = remote.start(&session, None, StartOptions::default()).await?.response()?;
        if !response.devices().contains(DeviceType::Pointer) {
            return Err(ashpd::Error::NoResponse);
        }
        let stream = response.streams().first().cloned().ok_or(ashpd::Error::NoResponse)?;
        let fd = screencast
            .open_pipe_wire_remote(&session, OpenPipeWireRemoteOptions::default())
            .await?;
        Ok::<_, ashpd::Error>((remote, session, stream, fd))
    });
    let (remote, session, stream, fd) = match portal {
        Ok(value) => value,
        Err(error) => {
            set_failure(shared, format!("Could not start Wayland RemoteDesktop: {error}"));
            return;
        }
    };
    let node_id = stream.pipe_wire_node_id();
    let logical = stream.size().unwrap_or((0, 0));
    mark_portal_ready(shared, true, node_id, logical.0, logical.1);
    let pointer = PointerPortal {
        runtime: Rc::new(runtime),
        remote: Rc::new(remote),
        session: Rc::new(session),
        node_id,
    };
    if let Err(error) = run_pipewire_loop(node_id, fd, shared, receiver, Some(pointer)) {
        set_failure(shared, error);
    }
    mark_stopped(shared);
}

struct PointerPortal {
    runtime: Rc<tokio::runtime::Runtime>,
    remote: Rc<RemoteDesktop>,
    session: Rc<ashpd::desktop::Session<RemoteDesktop>>,
    node_id: u32,
}

impl PointerPortal {
    fn move_absolute(&self, x: f64, y: f64) -> Result<(), String> {
        self.runtime
            .block_on(self.remote.notify_pointer_motion_absolute(
                &self.session,
                self.node_id,
                x,
                y,
                NotifyPointerMotionAbsoluteOptions::default(),
            ))
            .map_err(|error| format!("Wayland RemoteDesktop pointer motion failed: {error}"))
    }

    fn close(&self) {
        let _ = self.runtime.block_on(self.session.close());
    }
}

#[derive(Default)]
struct PipeWireUserData {
    format: spa::param::video::VideoInfoRaw,
}

#[allow(clippy::too_many_lines)] // Listener + format negotiation must share local PipeWire lifetimes.
fn run_pipewire_loop(
    node_id: u32,
    fd: std::os::fd::OwnedFd,
    shared: &Arc<SharedState>,
    receiver: pw::channel::Receiver<ControlMessage>,
    pointer: Option<PointerPortal>,
) -> Result<(), String> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .map_err(|error| format!("Could not create the PipeWire capture loop: {error}"))?;
    let context = pw::context::ContextRc::new(&mainloop, None)
        .map_err(|error| format!("Could not create the PipeWire capture context: {error}"))?;
    let core = context.connect_fd_rc(fd, None).map_err(|error| {
        format!("Could not connect PipeWire to the ScreenCast portal stream: {error}")
    })?;
    let stream = pw::stream::StreamRc::new(
        core,
        "SMU Wayland Screen Capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|error| format!("Could not create the PipeWire screen capture stream: {error}"))?;

    let loop_for_state = mainloop.clone();
    let shared_for_state = Arc::clone(shared);
    let shared_for_process = Arc::clone(shared);
    let _listener = stream
        .add_local_listener_with_user_data(PipeWireUserData::default())
        .state_changed(move |_, _, _, state| {
            if let pw::stream::StreamState::Error(error) = state {
                set_failure(
                    &shared_for_state,
                    format!("Wayland ScreenCast stream failed: {error}"),
                );
                loop_for_state.quit();
            }
        })
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            if spa::param::format_utils::parse_format(param).is_ok_and(
                |(media_type, media_subtype)| {
                    media_type == spa::param::format::MediaType::Video
                        && media_subtype == spa::param::format::MediaSubtype::Raw
                },
            ) {
                let _ = user_data.format.parse(param);
            }
        })
        .process(move |stream, user_data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else {
                return;
            };
            let chunk = data.chunk();
            let offset = usize::try_from(chunk.offset()).unwrap_or(usize::MAX);
            let size = usize::try_from(chunk.size()).unwrap_or(0);
            let stride = chunk.stride();
            let Some(bytes) = data.data() else {
                return;
            };
            let Some(end) = offset.checked_add(size) else {
                return;
            };
            let Some(source) = bytes.get(offset..end) else {
                return;
            };
            copy_frame(
                &shared_for_process,
                source,
                stride,
                user_data.format.format(),
                user_data.format.size().width,
                user_data.format.size().height,
            );
        })
        .register()
        .map_err(|error| format!("Could not register the PipeWire capture listener: {error}"))?;

    let loop_for_control = mainloop.clone();
    let pointer = pointer.map(Rc::new);
    let pointer_for_control = pointer.clone();
    let _control_receiver = receiver.attach(mainloop.loop_(), move |message| match message {
        ControlMessage::Stop => loop_for_control.quit(),
        ControlMessage::MovePointer { x, y, response } => {
            let result = pointer_for_control.as_ref().map_or_else(
                || Err("Wayland RemoteDesktop pointer control is not active.".to_owned()),
                |pointer| pointer.move_absolute(x, y),
            );
            let _ = response.send(result);
        }
    });

    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA
        ),
    );
    let serialized = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|error| format!("Could not build the PipeWire video format request: {error}"))?
    .0
    .into_inner();
    let pod = spa::pod::Pod::from_bytes(&serialized)
        .ok_or_else(|| "Could not build the PipeWire video format request.".to_owned())?;
    let mut params = [pod];
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|error| {
            format!("Could not connect to the selected PipeWire screen stream: {error}")
        })?;

    mainloop.run();
    if let Some(pointer) = pointer {
        pointer.close();
    }
    Ok(())
}

fn copy_frame(
    shared: &SharedState,
    source: &[u8],
    stride: i32,
    format: spa::param::video::VideoFormat,
    width: u32,
    height: u32,
) {
    let bgr = format == spa::param::video::VideoFormat::BGRx
        || format == spa::param::video::VideoFormat::BGRA;
    let rgb = format == spa::param::video::VideoFormat::RGBx
        || format == spa::param::video::VideoFormat::RGBA;
    if !bgr && !rgb {
        let mut state = lock(&shared.state);
        "Wayland ScreenCast selected an unsupported pixel format.".clone_into(&mut state.status);
        drop(state);
        return;
    }
    let Ok(width) = usize::try_from(width) else {
        return;
    };
    let Ok(height) = usize::try_from(height) else {
        return;
    };
    if width == 0 || height == 0 {
        return;
    }
    let Some(row_bytes) = width.checked_mul(4) else {
        return;
    };
    let source_stride = if stride > 0 { usize::try_from(stride).unwrap_or(0) } else { row_bytes };
    let Some(required) = source_stride.checked_mul(height) else {
        return;
    };
    if source_stride < row_bytes || source.len() < required {
        let mut state = lock(&shared.state);
        "Wayland ScreenCast delivered an incomplete frame.".clone_into(&mut state.status);
        drop(state);
        return;
    }
    let Some(target_size) = width.checked_mul(height).and_then(|value| value.checked_mul(3)) else {
        return;
    };
    let mut frame = vec![0_u8; target_size];
    for y in 0..height {
        let source_row = &source[y * source_stride..y * source_stride + row_bytes];
        for x in 0..width {
            let pixel = &source_row[x * 4..x * 4 + 4];
            let target = (y * width + x) * 3;
            frame[target] = pixel[if bgr { 2 } else { 0 }];
            frame[target + 1] = pixel[1];
            frame[target + 2] = pixel[if bgr { 0 } else { 2 }];
        }
    }
    let Ok(frame_width) = i32::try_from(width) else {
        return;
    };
    let Ok(frame_height) = i32::try_from(height) else {
        return;
    };
    let mut state = lock(&shared.state);
    state.frame = frame;
    state.frame_width = frame_width;
    state.frame_height = frame_height;
    if state.logical_width <= 0 || state.logical_height <= 0 {
        state.logical_width = frame_width;
        state.logical_height = frame_height;
    }
    drop(state);
    shared.changed.notify_all();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn script_activation_decline_unblocks_waiter_with_cpp_message() {
        let portal = Arc::new(WaylandPortalCapture::default());
        let waiter = Arc::clone(&portal);
        let thread = std::thread::spawn(move || waiter.request_activation_for_script(&|| false));
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !portal.has_pending_activation_request() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(portal.has_pending_activation_request());
        portal.decline_activation_request();
        let error = thread
            .join()
            .expect("activation waiter should finish")
            .expect_err("decline should fail activation");
        assert_eq!(
            error.to_string(),
            "Wayland screen capture was declined. getPixelColor and getPixelRect require selecting a monitor."
        );
    }

    #[test]
    fn conflicting_activation_kind_is_rejected_while_prompt_is_pending() {
        let portal = Arc::new(WaylandPortalCapture::default());
        let waiter = Arc::clone(&portal);
        let thread = std::thread::spawn(move || waiter.request_activation_for_script(&|| false));
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !portal.has_pending_activation_request() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        let error = portal
            .request_remote_desktop_activation_for_script(&|| false)
            .expect_err("different activation kind should conflict");
        assert_eq!(
            error.to_string(),
            "another Wayland portal permission request is already awaiting a decision"
        );
        portal.decline_activation_request();
        let _ = thread.join();
    }

    #[test]
    fn cancelled_script_wait_does_not_resolve_shared_prompt() {
        let portal = WaylandPortalCapture::default();
        let error = portal
            .request_activation_for_script(&|| true)
            .expect_err("cancelled script should stop waiting");
        assert_eq!(
            error.to_string(),
            "script was stopped while waiting for Wayland portal permission"
        );
        assert!(portal.has_pending_activation_request());
        portal.decline_activation_request();
    }

    #[test]
    fn pointer_mapping_matches_cpp_endpoint_math() {
        assert!(map_pointer_coordinate(0, 1920, 1280).abs() < f64::EPSILON);
        assert!((map_pointer_coordinate(1919, 1920, 1280) - 1279.0).abs() < f64::EPSILON);
        assert!((map_pointer_coordinate(960, 1920, 1280) - 639.833_246_482_543).abs() < 1e-9);
        assert!(map_pointer_coordinate(0, 1, 1).abs() < f64::EPSILON);
    }

    #[test]
    fn frame_copy_converts_bgrx_to_rgb_and_honors_stride() {
        let shared = SharedState::default();
        let source =
            [3, 2, 1, 0, 6, 5, 4, 0, 99, 99, 99, 99, 9, 8, 7, 0, 12, 11, 10, 0, 88, 88, 88, 88];
        copy_frame(&shared, &source, 12, spa::param::video::VideoFormat::BGRx, 2, 2);
        let state = lock(&shared.state);
        assert_eq!(state.frame_width, 2);
        assert_eq!(state.frame_height, 2);
        assert_eq!(state.frame, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn frame_copy_converts_rgba_to_rgb() {
        let shared = SharedState::default();
        copy_frame(
            &shared,
            &[1, 2, 3, 255, 4, 5, 6, 255],
            8,
            spa::param::video::VideoFormat::RGBA,
            2,
            1,
        );
        assert_eq!(lock(&shared.state).frame, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn incomplete_frame_keeps_previous_pixels_and_sets_status() {
        let shared = SharedState::default();
        {
            let mut state = lock(&shared.state);
            state.frame = vec![1, 2, 3];
        }
        copy_frame(&shared, &[0; 4], 8, spa::param::video::VideoFormat::RGBx, 2, 1);
        let state = lock(&shared.state);
        assert_eq!(state.frame, [1, 2, 3]);
        assert_eq!(state.status, "Wayland ScreenCast delivered an incomplete frame.");
    }
}

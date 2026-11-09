//! `zwp_text_input_v3` (server side) bridged to the widget's `GtkIMContext`.
//!
//! The embedded client enables text input → its [`SurfaceView`] attaches a
//! `GtkIMMulticontext` to its key controller, so whatever input method
//! hyprwidget itself uses (fcitx5 via its GTK module or text-input on
//! Hyprland) handles the keys. Preedit/commit/delete-surrounding come back
//! from the IM context and are sent to the client with one `done`.
//!
//! smithay's own `text_input` only forwards to an `input_method_v2`
//! client, so the protocol is implemented here.

use std::sync::Mutex;

use gtk4::{InputHints, InputPurpose};
use smithay::reexports::wayland_protocols::wp::text_input::zv3::server::zwp_text_input_manager_v3::{
    self, ZwpTextInputManagerV3,
};
use smithay::reexports::wayland_protocols::wp::text_input::zv3::server::zwp_text_input_v3::{
    self, ContentHint, ContentPurpose, ZwpTextInputV3,
};
use smithay::reexports::wayland_server::backend::{ClientId, GlobalId, ObjectId};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};

use crate::server::{EmbedServer, State, root_of, tree_offset};
use crate::surface::{ImBatch, Rect, TextInputInfo};

pub(crate) struct TextInputState {
    _global: GlobalId,
    inputs: Vec<ZwpTextInputV3>,
}

impl TextInputState {
    pub(crate) fn new(dh: &DisplayHandle) -> Self {
        Self {
            _global: dh.create_global::<State, ZwpTextInputManagerV3, ()>(1, ()),
            inputs: Vec::new(),
        }
    }
}

/// Double-buffered state of one `zwp_text_input_v3`.
#[derive(Default)]
pub(crate) struct TextInputData(Mutex<Ti>);

#[derive(Default, Clone)]
struct TiState {
    surrounding: Option<(String, i32, i32)>,
    rect: Option<(i32, i32, i32, i32)>,
    content: (u32, u32),
}

#[derive(Default)]
struct Ti {
    pending_enable: Option<bool>,
    pending: TiState,
    enabled: bool,
    current: TiState,
    /// Number of `commit` requests (the serial of `done`).
    commits: u32,
    /// Surface this text input has entered.
    focus: Option<WlSurface>,
}

fn purpose(p: u32) -> InputPurpose {
    match p {
        1 => InputPurpose::Alpha,
        2 => InputPurpose::Digits,
        3 => InputPurpose::Number,
        4 => InputPurpose::Phone,
        5 => InputPurpose::Url,
        6 => InputPurpose::Email,
        7 => InputPurpose::Name,
        8 => InputPurpose::Password,
        9 => InputPurpose::Pin,
        13 => InputPurpose::Terminal,
        _ => InputPurpose::FreeForm,
    }
}

fn hints(h: u32) -> InputHints {
    let map = [
        (ContentHint::Completion, InputHints::WORD_COMPLETION),
        (ContentHint::Spellcheck, InputHints::SPELLCHECK),
        (
            ContentHint::AutoCapitalization,
            InputHints::UPPERCASE_SENTENCES,
        ),
        (ContentHint::Lowercase, InputHints::LOWERCASE),
        (ContentHint::Uppercase, InputHints::UPPERCASE_CHARS),
        (ContentHint::Titlecase, InputHints::UPPERCASE_WORDS),
        (ContentHint::HiddenText, InputHints::PRIVATE),
        (ContentHint::SensitiveData, InputHints::PRIVATE),
    ];
    let h = ContentHint::from_bits_truncate(h);
    map.iter()
        .filter(|(c, _)| h.contains(*c))
        .fold(InputHints::NONE, |acc, (_, g)| acc | *g)
}

fn same_client(a: &impl Resource, b: &impl Resource) -> bool {
    a.client().map(|c| c.id()) == b.client().map(|c| c.id())
}

impl State {
    /// Keyboard focus moved: `leave`/`enter` the text inputs and drop the
    /// IM bridge of the old widget.
    pub(crate) fn text_input_focus(&mut self, old: Option<&WlSurface>, new: Option<&WlSurface>) {
        for ti in &self.text_input.inputs {
            let data = ti.data::<TextInputData>().unwrap();
            let mut d = data.0.lock().unwrap();
            if let Some(o) = d.focus.take()
                && o.is_alive()
            {
                ti.leave(&o);
            }
            if let Some(n) = new
                && same_client(ti, n)
            {
                ti.enter(n);
                d.focus = Some(n.clone());
            }
        }
        let new_sv = new.and_then(|n| self.sv_of(n));
        if let Some(sv) = old.and_then(|o| self.sv_of(o))
            && new_sv.as_ref() != Some(&sv)
        {
            sv.set_text_input(None);
        }
    }

    fn text_input_committed(&mut self, ti: &ZwpTextInputV3) {
        let (focus, info) = {
            let d = ti.data::<TextInputData>().unwrap().0.lock().unwrap();
            let Some(focus) = d.focus.clone() else {
                return;
            };
            let info = d.enabled.then(|| {
                let (ox, oy) = tree_offset(&focus);
                TextInputInfo {
                    rect: d.current.rect.map(|(x, y, w, h)| Rect {
                        x: x + ox,
                        y: y + oy,
                        w,
                        h,
                    }),
                    surrounding: d.current.surrounding.clone(),
                    purpose: purpose(d.current.content.1),
                    hints: hints(d.current.content.0),
                }
            });
            (focus, info)
        };
        if let Some(sv) = self.sv_of(&focus) {
            sv.set_text_input(info);
        }
    }
}

impl EmbedServer {
    /// IM output from the widget showing `root`.
    pub(crate) fn text_input_send(&self, root: WlSurface, batch: ImBatch) {
        self.run(move |st| {
            let Some(focus) = st.kb_focus.clone() else {
                return;
            };
            if root_of(&focus) != root {
                return;
            }
            for ti in &st.text_input.inputs {
                let d = ti.data::<TextInputData>().unwrap().0.lock().unwrap();
                if !d.enabled || d.focus.as_ref() != Some(&focus) {
                    continue;
                }
                if let Some((before, after)) = batch.delete {
                    ti.delete_surrounding_text(before, after);
                }
                if let Some(text) = &batch.commit {
                    ti.commit_string(Some(text.clone()));
                }
                let (text, begin, end) = &batch.preedit;
                if !text.is_empty() {
                    ti.preedit_string(Some(text.clone()), *begin, *end);
                }
                ti.done(d.commits);
            }
        });
    }
}

impl GlobalDispatch<ZwpTextInputManagerV3, ()> for State {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ZwpTextInputManagerV3>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZwpTextInputManagerV3, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwpTextInputManagerV3,
        request: zwp_text_input_manager_v3::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwp_text_input_manager_v3::Request::GetTextInput { id, .. } = request {
            let ti = data_init.init(id, TextInputData::default());
            if let Some(f) = state.kb_focus.as_ref().filter(|f| same_client(&ti, *f)) {
                ti.enter(f);
                ti.data::<TextInputData>().unwrap().0.lock().unwrap().focus = Some(f.clone());
            }
            state.text_input.inputs.push(ti);
        }
    }
}

impl Dispatch<ZwpTextInputV3, TextInputData> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZwpTextInputV3,
        request: zwp_text_input_v3::Request,
        data: &TextInputData,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use zwp_text_input_v3::Request as R;
        // Only commits change what the widget sees.
        let committed = matches!(request, R::Commit);
        {
            let mut d = data.0.lock().unwrap();
            match request {
                R::Enable => {
                    // `enable` resets the pending state.
                    d.pending_enable = Some(true);
                    d.pending = TiState::default();
                }
                R::Disable => d.pending_enable = Some(false),
                R::SetSurroundingText {
                    text,
                    cursor,
                    anchor,
                } => d.pending.surrounding = Some((text, cursor, anchor)),
                R::SetContentType { hint, purpose } => {
                    let h = hint.into_result().map(|h| h.bits()).unwrap_or(0);
                    let p = purpose
                        .into_result()
                        .map(|p| p as u32)
                        .unwrap_or(ContentPurpose::Normal as u32);
                    d.pending.content = (h, p);
                }
                R::SetCursorRectangle {
                    x,
                    y,
                    width,
                    height,
                } => d.pending.rect = Some((x, y, width, height)),
                R::Commit => {
                    d.commits = d.commits.wrapping_add(1);
                    match d.pending_enable.take() {
                        Some(true) => {
                            d.enabled = true;
                            d.current = std::mem::take(&mut d.pending);
                        }
                        Some(false) => {
                            d.enabled = false;
                            d.current = TiState::default();
                            d.pending = TiState::default();
                        }
                        None => {
                            let p = std::mem::take(&mut d.pending);
                            if p.surrounding.is_some() {
                                d.current.surrounding = p.surrounding;
                            }
                            if p.rect.is_some() {
                                d.current.rect = p.rect;
                            }
                            if p.content != (0, 0) {
                                d.current.content = p.content;
                            }
                        }
                    }
                }
                R::Destroy => d.enabled = false,
                _ => {}
            }
        }
        if committed {
            state.text_input_committed(resource);
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        resource: &ZwpTextInputV3,
        data: &TextInputData,
    ) {
        let focus = data.0.lock().unwrap().focus.take();
        let id: ObjectId = resource.id();
        state.text_input.inputs.retain(|t| t.id() != id);
        if let Some(sv) = focus.and_then(|f| state.sv_of(&f)) {
            sv.set_text_input(None);
        }
    }
}

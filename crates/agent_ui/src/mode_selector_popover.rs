use std::rc::Rc;
use std::sync::Arc;

use acp_thread::AgentSessionModes;
use agent_client_protocol as acp;
use fs::Fs;
use gpui::Entity;
use picker::popover_menu::PickerPopoverMenu;
use ui::{PopoverMenuHandle, Tooltip, prelude::*};

use crate::{ModeSelector, ModeSelectorTooltip, mode_selector::acp_mode_selector};

pub struct ModeSelectorPopover {
    session_modes: Rc<dyn AgentSessionModes>,
    selector: Entity<ModeSelector>,
    menu_handle: PopoverMenuHandle<ModeSelector>,
}

impl ModeSelectorPopover {
    pub(crate) fn new(
        session_modes: Rc<dyn AgentSessionModes>,
        agent_server: Rc<dyn agent_servers::AgentServer>,
        fs: Arc<dyn Fs>,
        menu_handle: PopoverMenuHandle<ModeSelector>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_modes_clone = session_modes.clone();
        Self {
            session_modes,
            selector: cx.new(move |cx| {
                acp_mode_selector(session_modes_clone, agent_server, fs, window, cx)
            }),
            menu_handle,
        }
    }

    pub fn toggle(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_handle.toggle(window, cx);
    }

    pub fn current_mode(&self) -> acp::SessionModeId {
        self.session_modes.current_mode()
    }

    pub fn cycle_favorite_modes(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.selector.update(cx, |selector, cx| {
            selector.delegate.cycle_favorite_modes(window, cx);
        });
    }

    fn current_mode_name(&self) -> SharedString {
        let current_mode_id = self.session_modes.current_mode();
        self.session_modes
            .all_modes()
            .iter()
            .find(|mode| mode.id == current_mode_id)
            .map(|mode| mode.name.clone().into())
            .unwrap_or_else(|| "Unknown".into())
    }
}

impl Render for ModeSelectorPopover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode_name = self.current_mode_name();

        let (color, icon) = if self.menu_handle.is_deployed() {
            (Color::Accent, IconName::ChevronUp)
        } else {
            (Color::Muted, IconName::ChevronDown)
        };

        let show_cycle_row = self.selector.read(cx).delegate.favorites.len() > 1;

        let tooltip = Tooltip::element(move |_, _cx| {
            ModeSelectorTooltip::new()
                .show_cycle_row(show_cycle_row)
                .into_any_element()
        });

        PickerPopoverMenu::new(
            self.selector.clone(),
            Button::new("active-mode", mode_name)
                .label_size(LabelSize::Small)
                .color(color)
                .end_icon(Icon::new(icon).color(Color::Muted).size(IconSize::XSmall)),
            tooltip,
            gpui::Corner::BottomRight,
            cx,
        )
        .with_handle(self.menu_handle.clone())
        .render(window, cx)
    }
}

//! Opening a Vault file from Dropbox: connecting the account in the browser, browsing the whole
//! Dropbox, and disconnecting. Every Dropbox call runs off the UI thread.

use std::sync::Arc;

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, KeyDownEvent, SharedString, Task, Window,
    div, prelude::*, px,
};
use keefast_workspace::{
    ConnectCanceller, Dropbox, DropboxAccount, DropboxError, DropboxItem, VaultLocation,
};

use crate::strings;
use crate::theme::Theme;
use crate::widgets::{button, tool_button};

pub enum DropboxPanelEvent {
    /// The user picked a file to open.
    Picked(VaultLocation),
    Closed,
}

enum Account {
    /// Reading the secret store.
    Checking,
    Disconnected,
    /// Waiting for the user to allow keefast in their browser.
    Connecting {
        url: String,
        canceller: ConnectCanceller,
    },
    Connected(DropboxAccount),
}

pub struct DropboxPanel {
    dropbox: Arc<Dropbox>,
    account: Account,
    /// The folder shown, `""` for the top of the Dropbox.
    folder: String,
    items: Vec<DropboxItem>,
    loading: bool,
    error: Option<SharedString>,
    task: Option<Task<()>>,
    focus: FocusHandle,
}

impl EventEmitter<DropboxPanelEvent> for DropboxPanel {}

impl Focusable for DropboxPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DropboxPanel {
    pub fn new(dropbox: Arc<Dropbox>, cx: &mut Context<Self>) -> Self {
        let mut panel = Self {
            dropbox,
            account: Account::Checking,
            folder: String::new(),
            items: Vec::new(),
            loading: false,
            error: None,
            task: None,
            focus: cx.focus_handle(),
        };
        panel.run(
            cx,
            |dropbox| dropbox.account(),
            |this, account, cx| match account {
                Some(account) => this.connected(account, cx),
                None => this.account = Account::Disconnected,
            },
        );
        panel
    }

    /// Runs `call` off the UI thread, then `done` with its result, or shows its error.
    fn run<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        call: impl FnOnce(&Dropbox) -> Result<T, DropboxError> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let dropbox = Arc::clone(&self.dropbox);
        let call = cx
            .background_executor()
            .spawn(async move { call(&dropbox) });
        self.error = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(value) => done(this, value, cx),
                    Err(error) => this.fail(&error),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn fail(&mut self, error: &DropboxError) {
        match error {
            DropboxError::Cancelled => {}
            DropboxError::NotConnected | DropboxError::AccessRevoked => {
                self.account = Account::Disconnected;
                self.error = Some(describe(error).into());
            }
            _ => self.error = Some(describe(error).into()),
        }
        if let Account::Connecting { .. } | Account::Checking = self.account {
            self.account = Account::Disconnected;
        }
    }

    fn connected(&mut self, account: DropboxAccount, cx: &mut Context<Self>) {
        self.account = Account::Connected(account);
        self.open_folder(String::new(), cx);
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        let authorization = match self.dropbox.begin_connect() {
            Ok(authorization) => authorization,
            Err(error) => {
                self.fail(&error);
                cx.notify();
                return;
            }
        };
        let url = authorization.url().to_owned();
        cx.open_url(&url);
        self.account = Account::Connecting {
            url,
            canceller: authorization.canceller(),
        };
        self.run(
            cx,
            move |dropbox| dropbox.finish_connect(authorization),
            Self::connected,
        );
    }

    fn open_connect_page_again(&mut self, cx: &mut Context<Self>) {
        if let Account::Connecting { url, .. } = &self.account {
            cx.open_url(url);
        }
    }

    /// Stops waiting for the browser, if connecting.
    fn stop_connecting(&self) {
        if let Account::Connecting { canceller, .. } = &self.account {
            canceller.cancel();
        }
    }

    fn cancel_connect(&mut self, cx: &mut Context<Self>) {
        self.stop_connecting();
        self.account = Account::Disconnected;
        cx.notify();
    }

    fn disconnect(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.run(
            cx,
            |dropbox| dropbox.disconnect(),
            |this, (), _| {
                this.account = Account::Disconnected;
                this.items.clear();
                this.folder.clear();
            },
        );
    }

    fn open_folder(&mut self, folder: String, cx: &mut Context<Self>) {
        self.loading = true;
        let path = folder.clone();
        self.run(
            cx,
            move |dropbox| dropbox.list_folder(&path),
            move |this, items, _| {
                this.folder = folder;
                this.items = items;
            },
        );
    }

    fn open_parent(&mut self, cx: &mut Context<Self>) {
        let parent = match self.folder.rsplit_once('/') {
            Some((parent, _)) => parent.to_owned(),
            None => return,
        };
        self.open_folder(parent, cx);
    }

    fn pick(&mut self, item: &DropboxItem, cx: &mut Context<Self>) {
        match item.is_folder {
            true => self.open_folder(item.path.clone(), cx),
            false => cx.emit(DropboxPanelEvent::Picked(item.location())),
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.stop_connecting();
        cx.emit(DropboxPanelEvent::Closed);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => self.close(cx),
            "backspace" => self.open_parent(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_body(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        match &self.account {
            Account::Checking => div()
                .text_sm()
                .text_color(theme.muted)
                .child(strings::DROPBOX_CHECKING)
                .into_any_element(),
            Account::Disconnected => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().child(strings::DROPBOX_CONNECT_PROMPT))
                .child(
                    div().flex().child(
                        button("dropbox-connect", strings::DROPBOX_CONNECT, theme)
                            .bg(theme.accent)
                            .text_color(theme.accent_text)
                            .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
                    ),
                )
                .into_any_element(),
            Account::Connecting { .. } => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(div().child(strings::DROPBOX_CONNECTING))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            button("dropbox-open-page", strings::DROPBOX_OPEN_PAGE_AGAIN, theme)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.open_connect_page_again(cx)),
                                ),
                        )
                        .child(
                            button(
                                "dropbox-cancel-connect",
                                strings::DROPBOX_STOP_CONNECTING,
                                theme,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_connect(cx))),
                        ),
                )
                .into_any_element(),
            Account::Connected(account) => self.render_files(account, theme, cx),
        }
    }

    fn render_files(
        &self,
        account: &DropboxAccount,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let who = match account.email.as_str() {
            "" => account.name.clone(),
            email => email.to_owned(),
        };
        let folder: SharedString = match self.folder.as_str() {
            "" => strings::DROPBOX_TOP.into(),
            folder => folder.to_owned().into(),
        };
        let rows = self.items.iter().enumerate().map(|(ix, item)| {
            let vault_file = !item.is_folder && item.name.to_lowercase().ends_with(".kdbx");
            let icon = if item.is_folder {
                strings::DROPBOX_FOLDER_ICON
            } else {
                strings::DROPBOX_FILE_ICON
            };
            let picked = item.clone();
            div()
                .id(("dropbox-item", ix))
                .flex()
                .gap_2()
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|this| this.bg(theme.sidebar))
                .when(!item.is_folder && !vault_file, |this| {
                    this.text_color(theme.muted)
                })
                .child(div().flex_none().w(px(18.)).child(icon))
                .child(div().truncate().child(item.name.clone()))
                .on_click(cx.listener(move |this, _, _, cx| this.pick(&picked, cx)))
        });
        let empty = self.items.is_empty() && !self.loading;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .text_sm()
                    .text_color(theme.muted)
                    .child(
                        div()
                            .truncate()
                            .child(strings::DROPBOX_CONNECTED_AS.replace("{account}", &who)),
                    )
                    .child(
                        tool_button("dropbox-disconnect", strings::DROPBOX_DISCONNECT, theme)
                            .flex_none()
                            .on_click(cx.listener(|this, _, _, cx| this.disconnect(cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(!self.folder.is_empty(), |this| {
                        this.child(
                            tool_button("dropbox-up", strings::DROPBOX_UP, theme)
                                .flex_none()
                                .on_click(cx.listener(|this, _, _, cx| this.open_parent(cx))),
                        )
                    })
                    .child(
                        div()
                            .truncate()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(folder),
                    ),
            )
            .child(
                div()
                    .id("dropbox-items")
                    .h(px(260.))
                    .overflow_y_scroll()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .when(self.loading, |this| {
                        this.child(
                            div()
                                .px_3()
                                .py_1()
                                .text_sm()
                                .text_color(theme.muted)
                                .child(strings::DROPBOX_LOADING),
                        )
                    })
                    .when(empty, |this| {
                        this.child(
                            div()
                                .px_3()
                                .py_1()
                                .text_sm()
                                .text_color(theme.muted)
                                .child(strings::DROPBOX_EMPTY_FOLDER),
                        )
                    })
                    .when(!self.loading, |this| this.children(rows)),
            )
            .into_any_element()
    }
}

/// A panel closed while connecting stops waiting for the browser, freeing the loopback port.
impl Drop for DropboxPanel {
    fn drop(&mut self) {
        self.stop_connecting();
    }
}

impl Render for DropboxPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        div()
            .id("dropbox-panel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .w(px(460.))
            .flex()
            .flex_col()
            .gap_3()
            .p_6()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.panel)
            .text_color(theme.text)
            .child(div().text_xl().child(strings::DROPBOX_TITLE))
            .child(self.render_body(theme, cx))
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(theme.error).child(error))
            })
            .child(
                div().flex().justify_end().child(
                    button("dropbox-close", strings::CANCEL, theme)
                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                ),
            )
    }
}

/// What to tell the user about a Dropbox error.
pub fn describe(error: &DropboxError) -> &'static str {
    match error {
        DropboxError::NotConfigured => strings::DROPBOX_ERROR_NOT_CONFIGURED,
        DropboxError::NotConnected | DropboxError::AccessRevoked => {
            strings::DROPBOX_ERROR_NOT_CONNECTED
        }
        DropboxError::Cancelled | DropboxError::Refused(_) => strings::DROPBOX_ERROR_REFUSED,
        DropboxError::Callback(_) => strings::DROPBOX_ERROR_CALLBACK,
        DropboxError::Network(_) => strings::DROPBOX_ERROR_NETWORK,
        DropboxError::NotFound => strings::DROPBOX_ERROR_NOT_FOUND,
        DropboxError::Conflict => strings::DROPBOX_ERROR_CONFLICT,
        DropboxError::Api(_) => strings::DROPBOX_ERROR_API,
        DropboxError::SecretStore(_) => strings::DROPBOX_ERROR_SECRET_STORE,
    }
}

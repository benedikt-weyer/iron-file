# Iced patches

`iced-wgpu-native-backdrop-blur.patch` targets `iced_wgpu` 0.14.0. It adds a
sampleable surface snapshot for custom shader primitives that opt in through
`Primitive::needs_backdrop`. The renderer copies the previously composed frame
before each opt-in primitive, allowing the primitive's WGSL shader to apply a
Gaussian blur only within its bounds.

Iron File vendors the patched crate at `vendor/iced-wgpu` and selects it through
the workspace `[patch.crates-io]` override. The patch file remains here to make
the renderer change auditable and to support rebasing onto a future Iced release.

`iced-widget-scroll-controls.patch` targets `iced_widget` 0.14.2. It retains
the configured scroll step and smooth-scroll API used by Iron File, and provides
small compatibility helpers for conditional rows, columns, stacks, and space.

Iced 0.14 includes the upstream multiline text-alignment fix, so no
`iced_graphics` patch is needed.

`winit-no-csd-fallback.patch` targets `winit` 0.30.13's Wayland backend
(`src/platform_impl/linux/wayland/window/state.rs`). Iron File's Iced
frontend draws no titlebar/CSD of its own and wants the compositor's
xdg-decoration negotiation (`ServerSide`/`ClientSide`) to be the sole
authority over whether the window gets decorated. Stock winit asks for
`ServerSide` but falls back to drawing its own client-side frame (via
`sctk-adwaita`) whenever the compositor responds `ClientSide`, which shows
an unwanted title bar even when the compositor intentionally declines to
decorate. The patch adds a new `WindowState::csd_fallback: bool` (default
`false`), purely additive alongside the existing `decorate`/`csd_fails`
fields, and ANDs it into the fallback-frame condition, so a `ClientSide`
response simply means no decoration, matching
`iced::window::Settings { decorations: true, .. }` in
`crates/iced-frontend/src/main.rs`. Nothing currently flips `csd_fallback`
back to `true` — it exists so the stock behavior stays one field away if
ever needed, without re-deriving the removed code from scratch.

Iron File vendors the patched crate at `vendor/winit` and selects it through
the workspace `[patch.crates-io]` override, same as the Iced crates above.

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

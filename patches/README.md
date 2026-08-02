# Iced patches

`iced-wgpu-native-backdrop-blur.patch` targets `iced_wgpu` 0.13.5. It adds a
sampleable surface snapshot for custom shader primitives that opt in through
`Primitive::needs_backdrop`. The renderer copies the previously composed frame
before each opt-in primitive, allowing the primitive's WGSL shader to apply a
Gaussian blur only within its bounds.

Iron File vendors the patched crate at `vendor/iced-wgpu` and selects it through
the workspace `[patch.crates-io]` override. The patch file remains here to make
the renderer change auditable and to support rebasing onto a future Iced release.

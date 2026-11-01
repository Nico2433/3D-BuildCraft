//! Selection outline: the selected items drawn into a mask, then dilated. Both textures are
//! cached until geometry, placement, camera or viewport changes.

use super::*;

impl Renderer {
    pub(super) fn update_selection(
        &mut self,
        cb: &FrameCallback,
        device: &wgpu::Device,
        screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        // Egui can prepare an oversized viewport during its first layout pass.
        let size = std::array::from_fn(|i| {
            let limit = screen.size_in_pixels[i]
                .min(device.limits().max_texture_dimension_2d)
                .max(1);
            cb.viewport[i].round().clamp(1.0, limit as f32) as u32
        });
        let key = SelectionKey {
            view_proj: cb.view_proj,
            size,
            items: cb
                .items
                .iter()
                .filter(|it| it.selected)
                .map(|it| (Arc::as_ptr(&it.mesh) as usize, it.model))
                .collect(),
        };
        if key.items.is_empty() {
            self.selection = None;
        } else if self.selection.as_ref().is_none_or(|s| s.key != key) {
            if self
                .selection
                .as_ref()
                .is_none_or(|s| s.key.size != key.size)
            {
                let texture = |label| {
                    let view = device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some(label),
                            size: wgpu::Extent3d {
                                width: key.size[0],
                                height: key.size[1],
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::R8Unorm,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        })
                        .create_view(&Default::default());
                    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(label),
                        layout: &self.mask_layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        }],
                    });
                    (view, bg)
                };
                let (mask, mask_bg) = texture("selection mask");
                let (outline, outline_bg) = texture("selection outline");
                self.selection = Some(SelectionMask {
                    key: SelectionKey {
                        view_proj: Mat4::IDENTITY,
                        size: key.size,
                        items: vec![],
                    },
                    mask,
                    mask_bg,
                    outline,
                    outline_bg,
                });
            }
            // Limit dilation to the projected selection box, especially when a dense
            // model occupies only a small part of a large viewport.
            let size = glam::Vec2::new(key.size[0] as f32, key.size[1] as f32);
            let (mut min, mut max) = (size, glam::Vec2::ZERO);
            'bounds: for it in cb.items.iter().filter(|it| it.selected) {
                let b = self.meshes[&(Arc::as_ptr(&it.mesh) as usize)].bounds;
                let m = cb.view_proj * it.model;
                for x in [b.min.x, b.max.x] {
                    for y in [b.min.y, b.max.y] {
                        for z in [b.min.z, b.max.z] {
                            let p = m * Vec4::new(x, y, z, 1.0);
                            // A box crossing the eye plane can cover the whole screen.
                            if p.w <= 0.0 {
                                min = glam::Vec2::ZERO;
                                max = size;
                                break 'bounds;
                            }
                            let p = (p.truncate().truncate() / p.w * glam::Vec2::new(0.5, -0.5)
                                + glam::Vec2::splat(0.5))
                                * size;
                            min = min.min(p);
                            max = max.max(p);
                        }
                    }
                }
            }
            let min = (min - glam::Vec2::splat(5.0))
                .floor()
                .clamp(glam::Vec2::ZERO, size)
                .as_uvec2();
            let max = (max + glam::Vec2::splat(5.0))
                .ceil()
                .clamp(glam::Vec2::ZERO, size)
                .as_uvec2();
            let selection = self.selection.as_mut().unwrap();
            // Both passes are cached until geometry, placement, camera or viewport changes.
            for dilate in [false, true] {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("selection"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: if dilate {
                            &selection.outline
                        } else {
                            &selection.mask
                        },
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                if min.x >= max.x || min.y >= max.y {
                    continue;
                }
                pass.set_scissor_rect(min.x, min.y, max.x - min.x, max.y - min.y);
                if dilate {
                    pass.set_pipeline(&self.dilate);
                    pass.set_bind_group(0, &selection.mask_bg, &[]);
                    pass.draw(0..3, 0..1);
                } else {
                    pass.set_pipeline(&self.mask_pipeline);
                    pass.set_bind_group(0, &self.globals_bg, &[]);
                    for (i, it) in cb.items.iter().enumerate().filter(|(_, it)| it.selected) {
                        let gm = &self.meshes[&(Arc::as_ptr(&it.mesh) as usize)];
                        pass.set_bind_group(
                            1,
                            &self.object_bg,
                            &[(i as u64 * OBJECT_STRIDE) as u32],
                        );
                        pass.set_vertex_buffer(0, gm.vbuf.slice(..));
                        pass.set_index_buffer(gm.ibuf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..gm.index_count, 0, 0..1);
                    }
                }
            }
            selection.key = key;
        }
    }
}

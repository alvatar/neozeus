use super::*;
use crate::*;

pub(crate) fn create_terminal_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x.max(1),
            height: size.y.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[
            DEFAULT_BG.r(),
            DEFAULT_BG.g(),
            DEFAULT_BG.b(),
            DEFAULT_BG.a(),
        ],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.data = None;
    image.sampler = ImageSampler::nearest();
    image
}

fn dump_terminal_image_ppm(image: &Image, path: &Path) -> Result<(), String> {
    let width = image.texture_descriptor.size.width;
    let height = image.texture_descriptor.size.height;
    let data = image
        .data
        .as_ref()
        .ok_or_else(|| "image data missing".to_owned())?;
    let mut output = Vec::with_capacity((width as usize * height as usize * 3) + 64);
    output.extend_from_slice(format!("P6\n{} {}\n255\n", width, height).as_bytes());
    for pixel in data.chunks_exact(4) {
        output.extend_from_slice(&pixel[..3]);
    }
    fs::write(path, output).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

pub(crate) fn sync_terminal_texture(
    mut terminal_manager: ResMut<TerminalManager>,
    font_state: Res<TerminalFontState>,
    primary_window: Single<&Window, With<PrimaryWindow>>,
    upload_queue: Res<TerminalGpuUploadQueue>,
    mut glyph_cache: ResMut<TerminalGlyphCache>,
    mut images: ResMut<Assets<Image>>,
    mut text_renderer: ResMut<TerminalTextRenderer>,
) {
    if text_renderer.font_system.is_none() {
        append_debug_log("texture sync: no font system");
        return;
    }

    if font_state.is_changed() {
        append_debug_log("texture sync: font state changed, clearing glyph cache");
        glyph_cache.glyphs.clear();
    }

    let active_id = terminal_manager.active_id;
    for (terminal_id, terminal) in terminal_manager.terminals.iter_mut() {
        let Some(surface) = &terminal.latest.surface else {
            terminal.pending_damage = None;
            continue;
        };

        let Some(image_handle) = terminal.texture_state.image.clone() else {
            append_debug_log("texture sync: missing image handle");
            continue;
        };
        let Some(helper_entities) = terminal.texture_state.helper_entities else {
            append_debug_log("texture sync: missing helper entities");
            continue;
        };

        let pixel_perfect = Some(*terminal_id) == active_id
            && terminal.display_mode == TerminalDisplayMode::PixelPerfect;
        let desired_cell_size = if pixel_perfect {
            pixel_perfect_cell_size(surface.cols, surface.rows, &primary_window)
        } else {
            UVec2::new(DEFAULT_CELL_WIDTH_PX, DEFAULT_CELL_HEIGHT_PX)
        };
        if terminal.texture_state.cell_size != desired_cell_size {
            terminal.texture_state.cell_size = desired_cell_size;
        }

        let cell_size = terminal.texture_state.cell_size;
        let texture_size = UVec2::new(
            surface.cols as u32 * cell_size.x.max(1),
            surface.rows as u32 * cell_size.y.max(1),
        );
        let has_pending_surface = terminal.surface_revision != terminal.uploaded_revision;
        let mut full_redraw =
            font_state.is_changed() || terminal.texture_state.texture_size != texture_size;
        let mut dirty_rows = if full_redraw {
            (0..surface.rows).collect::<Vec<_>>()
        } else if has_pending_surface {
            match terminal
                .pending_damage
                .as_ref()
                .unwrap_or(&TerminalDamage::Full)
            {
                TerminalDamage::Full => {
                    full_redraw = true;
                    (0..surface.rows).collect::<Vec<_>>()
                }
                TerminalDamage::Rows(rows) => rows.clone(),
            }
        } else {
            Vec::new()
        };

        if dirty_rows.is_empty() {
            continue;
        }

        if let Some(target_image) = images.get_mut(&image_handle) {
            if target_image.texture_descriptor.size.width != texture_size.x
                || target_image.texture_descriptor.size.height != texture_size.y
            {
                *target_image = create_terminal_image(texture_size);
                terminal.texture_state.cpu_pixels = vec![
                    DEFAULT_BG.r(),
                    DEFAULT_BG.g(),
                    DEFAULT_BG.b(),
                    DEFAULT_BG.a(),
                ];
                terminal.texture_state.cpu_pixels.resize(
                    (texture_size.x * texture_size.y * 4) as usize,
                    DEFAULT_BG.a(),
                );
                for pixel in terminal.texture_state.cpu_pixels.chunks_exact_mut(4) {
                    pixel.copy_from_slice(&[
                        DEFAULT_BG.r(),
                        DEFAULT_BG.g(),
                        DEFAULT_BG.b(),
                        DEFAULT_BG.a(),
                    ]);
                }
                full_redraw = true;
                dirty_rows = (0..surface.rows).collect();
            }

            if terminal.texture_state.cpu_pixels.len()
                != (texture_size.x * texture_size.y * 4) as usize
            {
                terminal
                    .texture_state
                    .cpu_pixels
                    .resize((texture_size.x * texture_size.y * 4) as usize, 0);
                full_redraw = true;
                dirty_rows = (0..surface.rows).collect();
            }

            if full_redraw {
                clear_terminal_pixels(&mut terminal.texture_state.cpu_pixels);
            }

            let compose_started = std::time::Instant::now();
            repaint_terminal_pixels(
                &mut terminal.texture_state.cpu_pixels,
                texture_size.x,
                surface,
                &dirty_rows,
                cell_size,
                helper_entities,
                &mut text_renderer,
                &mut glyph_cache,
                &font_state,
            );
            let compose_elapsed = compose_started.elapsed();
            with_debug_stats(&terminal.bridge.debug_stats, |stats| {
                stats.compose_micros += compose_elapsed.as_micros() as u64;
                stats.dirty_rows_uploaded += dirty_rows.len() as u64;
            });
            queue_terminal_uploads(
                &upload_queue,
                &image_handle,
                texture_size,
                &terminal.texture_state.cpu_pixels,
                &dirty_rows,
            );
            if env::var_os("NEOZEUS_DUMP_TEXTURE").is_some() {
                target_image.data = Some(terminal.texture_state.cpu_pixels.clone());
                let _ = dump_terminal_image_ppm(target_image, Path::new(DEBUG_TEXTURE_DUMP_PATH));
                target_image.data = None;
            }
            terminal.texture_state.texture_size = texture_size;
            terminal.uploaded_revision = terminal.surface_revision;
            terminal.pending_damage = None;
        } else {
            append_debug_log("texture sync: target image missing in assets");
        }
    }
}

fn clear_terminal_pixels(buffer: &mut [u8]) {
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[
            DEFAULT_BG.r(),
            DEFAULT_BG.g(),
            DEFAULT_BG.b(),
            DEFAULT_BG.a(),
        ]);
    }
}

pub(crate) fn queue_terminal_uploads(
    upload_queue: &TerminalGpuUploadQueue,
    image: &Handle<Image>,
    texture_size: UVec2,
    pixels: &[u8],
    dirty_rows: &[usize],
) {
    if dirty_rows.is_empty() {
        return;
    }

    let bytes_per_row = texture_size.x * 4;
    let mut uploads = Vec::new();
    let mut index = 0;
    while index < dirty_rows.len() {
        let start_row = dirty_rows[index] as u32;
        let mut end_index = index + 1;
        while end_index < dirty_rows.len() && dirty_rows[end_index] == dirty_rows[end_index - 1] + 1
        {
            end_index += 1;
        }
        let end_row = dirty_rows[end_index - 1] as u32;
        let height = end_row - start_row + 1;
        let start = start_row as usize * bytes_per_row as usize;
        let end = (end_row as usize + 1) * bytes_per_row as usize;
        uploads.push(TerminalTextureUpload {
            image: image.clone(),
            origin_y: start_row,
            width: texture_size.x,
            height,
            bytes_per_row,
            data: pixels[start..end].to_vec(),
        });
        index = end_index;
    }

    upload_queue.push_uploads(uploads);
}

#[allow(
    clippy::too_many_arguments,
    reason = "terminal row repaint needs renderer/cache/font state together"
)]
fn repaint_terminal_pixels(
    buffer: &mut [u8],
    texture_width: u32,
    surface: &TerminalSurface,
    rows: &[usize],
    cell_size: UVec2,
    helper_entities: TerminalFontEntities,
    text_renderer: &mut TerminalTextRenderer,
    glyph_cache: &mut TerminalGlyphCache,
    font_state: &TerminalFontState,
) {
    let stride = texture_width as usize * 4;

    for &y in rows {
        if y >= surface.rows {
            continue;
        }

        for x in 0..surface.cols {
            let cell = surface.cell(x, y);
            let origin_x = x as u32 * cell_size.x;
            let origin_y = y as u32 * cell_size.y;
            fill_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y,
                cell_size.x,
                cell_size.y,
                cell.bg,
            );

            if cell.width == 0 || cell.content.is_empty() {
                continue;
            }

            let (font_role, _helper_entity, preserve_color) =
                select_terminal_font_role(&cell.content, font_state, helper_entities);
            let cache_key = TerminalGlyphCacheKey {
                content: cell.content.clone(),
                font_role,
                width_cells: cell.width,
                cell_width: cell_size.x,
                cell_height: cell_size.y,
            };

            if !glyph_cache.glyphs.contains_key(&cache_key) {
                let glyph = rasterize_terminal_glyph(
                    &cache_key,
                    font_role,
                    preserve_color,
                    text_renderer,
                    font_state,
                );
                glyph_cache.glyphs.insert(cache_key.clone(), glyph);
            }

            if let Some(glyph) = glyph_cache.glyphs.get(&cache_key) {
                blit_cached_glyph_in_buffer(buffer, stride, origin_x, origin_y, glyph, cell.fg);
            }
        }
    }

    if let Some(cursor) = &surface.cursor {
        if cursor.visible && rows.binary_search(&cursor.y).is_ok() {
            draw_cursor_in_buffer(buffer, stride, cursor, cell_size);
        }
    }
}

fn select_terminal_font_role(
    content: &TerminalCellContent,
    font_state: &TerminalFontState,
    helper_entities: TerminalFontEntities,
) -> (TerminalFontRole, Entity, bool) {
    if content.any_char(is_emoji_like) && font_state.emoji_font.is_some() {
        return (TerminalFontRole::Emoji, helper_entities.emoji, true);
    }

    if content.any_char(is_private_use_like) && font_state.private_use_font.is_some() {
        return (
            TerminalFontRole::PrivateUse,
            helper_entities.private_use,
            false,
        );
    }

    (TerminalFontRole::Primary, helper_entities.primary, false)
}

fn terminal_text_attrs<'a>(
    font_role: TerminalFontRole,
    font_state: &'a TerminalFontState,
) -> CtAttrs<'a> {
    let family = match font_role {
        TerminalFontRole::Primary => CtFamily::Monospace,
        TerminalFontRole::PrivateUse => terminal_font_family_name(font_state, "private-use")
            .map(CtFamily::Name)
            .unwrap_or(CtFamily::Monospace),
        TerminalFontRole::Emoji => terminal_font_family_name(font_state, "emoji")
            .map(CtFamily::Name)
            .unwrap_or(CtFamily::Monospace),
    };
    CtAttrs::new().family(family)
}

fn terminal_font_family_name<'a>(
    font_state: &'a TerminalFontState,
    needle: &str,
) -> Option<&'a str> {
    let report = font_state.report.as_ref()?.as_ref().ok()?;
    report
        .fallbacks
        .iter()
        .find(|face| face.source.contains(needle))
        .map(|face| face.family.as_str())
}

pub(crate) fn rasterize_terminal_glyph(
    cache_key: &TerminalGlyphCacheKey,
    font_role: TerminalFontRole,
    preserve_color: bool,
    text_renderer: &mut TerminalTextRenderer,
    font_state: &TerminalFontState,
) -> CachedTerminalGlyph {
    let width = cache_key.cell_width * u32::from(cache_key.width_cells.max(1));
    let height = cache_key.cell_height.max(1);
    let mut pixels = vec![0; (width * height * 4) as usize];

    let Some(font_system) = text_renderer.font_system.as_mut() else {
        return CachedTerminalGlyph {
            width,
            height,
            pixels,
            preserve_color,
        };
    };

    let metrics = CtMetrics::new(height as f32 * 0.9, height as f32);
    let mut buffer = CtBuffer::new_empty(metrics);
    {
        let mut borrowed = buffer.borrow_with(font_system);
        borrowed.set_size(Some(width as f32), Some(height as f32));
        let attrs = terminal_text_attrs(font_role, font_state).metrics(metrics);
        let text = cache_key.content.to_owned_string();
        borrowed.set_text(text.as_str(), &attrs, CtShaping::Advanced, None);
        borrowed.shape_until_scroll(false);
    }

    let base_color = CtColor::rgb(0xFF, 0xFF, 0xFF);
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, run.line_y), 1.0);
            text_renderer.swash_cache.with_pixels(
                font_system,
                physical.cache_key,
                base_color,
                |x, y, color| {
                    let rgba = color.as_rgba();
                    let source = if preserve_color {
                        rgba
                    } else {
                        [255, 255, 255, rgba[3]]
                    };
                    let target_x = physical.x + x;
                    let target_y = physical.y + y;
                    if target_x < 0
                        || target_y < 0
                        || target_x >= width as i32
                        || target_y >= height as i32
                    {
                        return;
                    }
                    blend_over_pixel(&mut pixels, width, target_x as u32, target_y as u32, source);
                },
            );
        }
    }

    CachedTerminalGlyph {
        width,
        height,
        pixels,
        preserve_color,
    }
}

fn blit_cached_glyph_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    origin_x: u32,
    origin_y: u32,
    glyph: &CachedTerminalGlyph,
    fg: egui::Color32,
) {
    let max_height = buffer.len() / stride;
    for y in 0..glyph.height as usize {
        let target_y = origin_y as usize + y;
        if target_y >= max_height {
            break;
        }
        let dst_row = &mut buffer[target_y * stride..(target_y + 1) * stride];
        let src_row =
            &glyph.pixels[y * glyph.width as usize * 4..(y + 1) * glyph.width as usize * 4];
        for x in 0..glyph.width as usize {
            let src = &src_row[x * 4..x * 4 + 4];
            if src[3] == 0 {
                continue;
            }

            let source = if glyph.preserve_color {
                [src[0], src[1], src[2], src[3]]
            } else {
                [fg.r(), fg.g(), fg.b(), src[3]]
            };
            let dst_start = (origin_x as usize + x) * 4;
            if dst_start + 4 > dst_row.len() {
                break;
            }
            blend_rgba_in_place(&mut dst_row[dst_start..dst_start + 4], source);
        }
    }
}

fn fill_rect_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: egui::Color32,
) {
    let pixel = [color.r(), color.g(), color.b(), color.a()];
    let max_height = buffer.len() / stride;
    for row in y as usize..(y as usize).saturating_add(height as usize).min(max_height) {
        let row_slice = &mut buffer[row * stride..(row + 1) * stride];
        let start = x as usize * 4;
        let end = ((x + width) as usize * 4).min(row_slice.len());
        if start >= end {
            continue;
        }
        for dst in row_slice[start..end].chunks_exact_mut(4) {
            dst.copy_from_slice(&pixel);
        }
    }
}

fn draw_cursor_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    cursor: &TerminalCursor,
    cell_size: UVec2,
) {
    let origin_x = cursor.x as u32 * cell_size.x;
    let origin_y = cursor.y as u32 * cell_size.y;
    let color = [cursor.color.r(), cursor.color.g(), cursor.color.b(), 160];

    match cursor.shape {
        TerminalCursorShape::Block => {
            fill_alpha_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y,
                cell_size.x,
                cell_size.y,
                color,
            );
        }
        TerminalCursorShape::Underline => {
            let height = (cell_size.y / 8).max(1);
            fill_alpha_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y + cell_size.y.saturating_sub(height),
                cell_size.x,
                height,
                [cursor.color.r(), cursor.color.g(), cursor.color.b(), 255],
            );
        }
        TerminalCursorShape::Beam => {
            let width = (cell_size.x / 10).max(1);
            fill_alpha_rect_in_buffer(
                buffer,
                stride,
                origin_x,
                origin_y,
                width,
                cell_size.y,
                [cursor.color.r(), cursor.color.g(), cursor.color.b(), 255],
            );
        }
    }
}

fn fill_alpha_rect_in_buffer(
    buffer: &mut [u8],
    stride: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: [u8; 4],
) {
    let max_height = buffer.len() / stride;
    for row in y as usize..(y as usize).saturating_add(height as usize).min(max_height) {
        let row_slice = &mut buffer[row * stride..(row + 1) * stride];
        let start = x as usize * 4;
        let end = ((x + width) as usize * 4).min(row_slice.len());
        if start >= end {
            continue;
        }
        for dst in row_slice[start..end].chunks_exact_mut(4) {
            blend_rgba_in_place(dst, color);
        }
    }
}

fn blend_over_pixel(buffer: &mut [u8], width: u32, x: u32, y: u32, source: [u8; 4]) {
    let index = ((y * width + x) * 4) as usize;
    blend_rgba_in_place(&mut buffer[index..index + 4], source);
}

pub(crate) fn blend_rgba_in_place(dst: &mut [u8], source: [u8; 4]) {
    let src_alpha = source[3] as f32 / 255.0;
    let dst_alpha = dst[3] as f32 / 255.0;
    let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);

    if out_alpha <= f32::EPSILON {
        dst.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }

    for channel in 0..3 {
        let src = source[channel] as f32 / 255.0;
        let dst_value = dst[channel] as f32 / 255.0;
        let out = (src * src_alpha + dst_value * dst_alpha * (1.0 - src_alpha)) / out_alpha;
        dst[channel] = (out * 255.0).round() as u8;
    }

    dst[3] = (out_alpha * 255.0).round() as u8;
}

pub(crate) const HUD_SIDE_RESERVED: f32 = 72.0;
pub(crate) const HUD_TOP_RESERVED: f32 = 140.0;
pub(crate) const HUD_BOTTOM_RESERVED: f32 = 64.0;
pub(crate) const HUD_FRAME_PADDING: Vec2 = Vec2::new(18.0, 18.0);

pub(crate) fn flush_terminal_gpu_uploads(
    upload_queue: Res<TerminalGpuUploadQueue>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    render_queue: Res<RenderQueue>,
) {
    let mut pending = upload_queue.take_pending();
    let mut deferred = VecDeque::new();

    while let Some(upload) = pending.pop_front() {
        let Some(gpu_image) = gpu_images.get(&upload.image) else {
            deferred.push_back(upload);
            continue;
        };
        render_queue.write_texture(
            TexelCopyTextureInfo {
                texture: &gpu_image.texture,
                mip_level: 0,
                origin: Origin3d {
                    x: 0,
                    y: upload.origin_y,
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            &upload.data,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(upload.bytes_per_row),
                rows_per_image: None,
            },
            Extent3d {
                width: upload.width,
                height: upload.height,
                depth_or_array_layers: 1,
            },
        );
    }

    upload_queue.prepend_pending(deferred);
}

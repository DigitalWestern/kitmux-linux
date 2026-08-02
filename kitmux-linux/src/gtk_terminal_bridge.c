#include "gtk_terminal_bridge.h"

#include <epoxy/gl.h>

typedef struct {
  GtkGLArea parent_instance;
} KitmuxProductTerminalArea;

typedef struct {
  GtkGLAreaClass parent_class;
} KitmuxProductTerminalAreaClass;

G_DEFINE_TYPE(KitmuxProductTerminalArea, kitmux_product_terminal_area,
              GTK_TYPE_GL_AREA)

static void kitmux_product_terminal_area_class_init(
    KitmuxProductTerminalAreaClass *class) {
  gtk_widget_class_set_accessible_role(GTK_WIDGET_CLASS(class),
                                       GTK_ACCESSIBLE_ROLE_TERMINAL);
}

static void kitmux_product_terminal_area_init(KitmuxProductTerminalArea *area) {
  (void)area;
}

typedef struct {
  GLint program, vertex_array, array_buffer, element_array_buffer;
  GLint draw_framebuffer, read_framebuffer, renderbuffer, active_texture;
  GLint texture_2d[4], viewport[4], scissor_box[4];
  GLint blend_src_rgb, blend_dst_rgb, blend_src_alpha, blend_dst_alpha;
  GLint blend_equation_rgb, blend_equation_alpha;
  GLboolean blend, scissor, depth_test, stencil_test, cull_face, depth_mask;
  GLboolean color_mask[4];
} kitmux_gl_state;

GtkWidget *kitmux_product_terminal_area_new(void) {
  return g_object_new(kitmux_product_terminal_area_get_type(), NULL);
}

static void capture_gl_state(kitmux_gl_state *state) {
  glGetIntegerv(GL_CURRENT_PROGRAM, &state->program);
  glGetIntegerv(GL_VERTEX_ARRAY_BINDING, &state->vertex_array);
  glGetIntegerv(GL_ARRAY_BUFFER_BINDING, &state->array_buffer);
  glGetIntegerv(GL_ELEMENT_ARRAY_BUFFER_BINDING, &state->element_array_buffer);
  glGetIntegerv(GL_DRAW_FRAMEBUFFER_BINDING, &state->draw_framebuffer);
  glGetIntegerv(GL_READ_FRAMEBUFFER_BINDING, &state->read_framebuffer);
  glGetIntegerv(GL_RENDERBUFFER_BINDING, &state->renderbuffer);
  glGetIntegerv(GL_ACTIVE_TEXTURE, &state->active_texture);
  for (int unit = 0; unit < 4; ++unit) {
    glActiveTexture((GLenum)(GL_TEXTURE0 + unit));
    glGetIntegerv(GL_TEXTURE_BINDING_2D, &state->texture_2d[unit]);
  }
  glActiveTexture((GLenum)state->active_texture);
  glGetIntegerv(GL_VIEWPORT, state->viewport);
  glGetIntegerv(GL_SCISSOR_BOX, state->scissor_box);
  glGetIntegerv(GL_BLEND_SRC_RGB, &state->blend_src_rgb);
  glGetIntegerv(GL_BLEND_DST_RGB, &state->blend_dst_rgb);
  glGetIntegerv(GL_BLEND_SRC_ALPHA, &state->blend_src_alpha);
  glGetIntegerv(GL_BLEND_DST_ALPHA, &state->blend_dst_alpha);
  glGetIntegerv(GL_BLEND_EQUATION_RGB, &state->blend_equation_rgb);
  glGetIntegerv(GL_BLEND_EQUATION_ALPHA, &state->blend_equation_alpha);
  state->blend = glIsEnabled(GL_BLEND);
  state->scissor = glIsEnabled(GL_SCISSOR_TEST);
  state->depth_test = glIsEnabled(GL_DEPTH_TEST);
  state->stencil_test = glIsEnabled(GL_STENCIL_TEST);
  state->cull_face = glIsEnabled(GL_CULL_FACE);
  glGetBooleanv(GL_DEPTH_WRITEMASK, &state->depth_mask);
  glGetBooleanv(GL_COLOR_WRITEMASK, state->color_mask);
}

static void set_capability(GLenum capability, GLboolean enabled) {
  enabled ? glEnable(capability) : glDisable(capability);
}

static void restore_gl_state(const kitmux_gl_state *state) {
  glUseProgram((GLuint)state->program);
  glBindVertexArray((GLuint)state->vertex_array);
  glBindBuffer(GL_ARRAY_BUFFER, (GLuint)state->array_buffer);
  glBindBuffer(GL_ELEMENT_ARRAY_BUFFER, (GLuint)state->element_array_buffer);
  glBindFramebuffer(GL_DRAW_FRAMEBUFFER, (GLuint)state->draw_framebuffer);
  glBindFramebuffer(GL_READ_FRAMEBUFFER, (GLuint)state->read_framebuffer);
  glBindRenderbuffer(GL_RENDERBUFFER, (GLuint)state->renderbuffer);
  for (int unit = 0; unit < 4; ++unit) {
    glActiveTexture((GLenum)(GL_TEXTURE0 + unit));
    glBindTexture(GL_TEXTURE_2D, (GLuint)state->texture_2d[unit]);
  }
  glActiveTexture((GLenum)state->active_texture);
  glViewport(state->viewport[0], state->viewport[1], state->viewport[2],
             state->viewport[3]);
  glScissor(state->scissor_box[0], state->scissor_box[1],
            state->scissor_box[2], state->scissor_box[3]);
  glBlendFuncSeparate((GLenum)state->blend_src_rgb,
                      (GLenum)state->blend_dst_rgb,
                      (GLenum)state->blend_src_alpha,
                      (GLenum)state->blend_dst_alpha);
  glBlendEquationSeparate((GLenum)state->blend_equation_rgb,
                          (GLenum)state->blend_equation_alpha);
  set_capability(GL_BLEND, state->blend);
  set_capability(GL_SCISSOR_TEST, state->scissor);
  set_capability(GL_DEPTH_TEST, state->depth_test);
  set_capability(GL_STENCIL_TEST, state->stencil_test);
  set_capability(GL_CULL_FACE, state->cull_face);
  glDepthMask(state->depth_mask);
  glColorMask(state->color_mask[0], state->color_mask[1],
              state->color_mask[2], state->color_mask[3]);
}

uint32_t kitmux_gdk_base_layout_keyval(GtkWidget *widget,
                                       GtkEventController *controller,
                                       uint32_t keycode) {
  GdkDisplay *display = gtk_widget_get_display(widget);
  GdkEvent *event = gtk_event_controller_get_current_event(controller);
  int group = event ? gdk_key_event_get_layout(event) : 0;
  guint keyval = 0;
  if (!display || !gdk_display_translate_key(display, keycode,
                                              GDK_NO_MODIFIER_MASK, group,
                                              &keyval, NULL, NULL, NULL)) {
    return 0;
  }
  return keyval;
}

double kitmux_widget_surface_scale(GtkWidget *widget) {
  GtkNative *native = gtk_widget_get_native(widget);
  GdkSurface *surface = native ? gtk_native_get_surface(native) : NULL;
  if (surface) return gdk_surface_get_scale(surface);
  int factor = gtk_widget_get_scale_factor(widget);
  return factor > 0 ? (double)factor : 1.0;
}

bool kitmux_session_draw_preserving_gl_state(kitty_session *session) {
  if (!session) return false;
  kitmux_gl_state state;
  capture_gl_state(&state);
  bool drawn = kitty_session_draw(session);
  restore_gl_state(&state);
  return drawn;
}

bool kitmux_terminal_render_frame(kitty_engine *engine, kitty_session *session,
                                  double buffer_scale,
                                  int previous_width, int previous_height,
                                  kitmux_render_result *result,
                                  char *errbuf, size_t errbuf_len) {
  if (!engine || !session || !result || !errbuf || errbuf_len == 0) {
    return false;
  }
  kitmux_gl_state state;
  capture_gl_state(&state);
  *result = (kitmux_render_result){
      .framebuffer_width = state.viewport[2],
      .framebuffer_height = state.viewport[3],
      .render_scale = kitty_render_scale(engine),
  };
  if (result->framebuffer_width <= 0 || result->framebuffer_height <= 0) {
    restore_gl_state(&state);
    return true;
  }
  result->metrics_changed = result->render_scale != buffer_scale;
  if (result->metrics_changed &&
      !kitty_render_set_scale(engine, buffer_scale, &result->cell_width,
                              &result->cell_height, errbuf, errbuf_len)) {
    restore_gl_state(&state);
    return false;
  }
  result->viewport_changed = result->metrics_changed ||
      result->framebuffer_width != previous_width ||
      result->framebuffer_height != previous_height;
  glViewport(0, 0, result->framebuffer_width, result->framebuffer_height);
  glDisable(GL_SCISSOR_TEST);
  glClearColor(0.04f, 0.05f, 0.07f, 1.0f);
  glClear(GL_COLOR_BUFFER_BIT);
  bool ok = true;
  if (result->viewport_changed) {
    ok = kitty_session_set_viewport(session, result->framebuffer_width,
                                    result->framebuffer_height);
    if (!ok) g_strlcpy(errbuf, "libkitty rejected the framebuffer", errbuf_len);
  }
  if (ok) {
    ok = kitty_session_draw(session);
    if (!ok) g_strlcpy(errbuf, "libkitty failed to draw", errbuf_len);
  }
  restore_gl_state(&state);
  return ok;
}

bool kitmux_terminal_render_regions(kitty_engine *engine, double buffer_scale,
                                    kitmux_terminal_region *regions,
                                    size_t region_count, int *cell_width,
                                    int *cell_height, char *errbuf,
                                    size_t errbuf_len) {
  if (!engine || !regions || region_count == 0 || region_count > 64 ||
      !cell_width || !cell_height || !errbuf || errbuf_len == 0) {
    return false;
  }
  kitmux_gl_state state;
  capture_gl_state(&state);
  if (kitty_render_scale(engine) != buffer_scale &&
      !kitty_render_set_scale(engine, buffer_scale, cell_width, cell_height,
                              errbuf, errbuf_len)) {
    restore_gl_state(&state);
    return false;
  }
  glViewport(0, 0, state.viewport[2], state.viewport[3]);
  glDisable(GL_SCISSOR_TEST);
  glClearColor(0.04f, 0.05f, 0.07f, 1.0f);
  glClear(GL_COLOR_BUFFER_BIT);
  bool ok = true;
  for (size_t i = 0; i < region_count; i++) {
    kitmux_terminal_region *region = &regions[i];
    if (!region->session || region->width <= 0 || region->height <= 0) {
      ok = false;
      g_strlcpy(errbuf, "invalid terminal region", errbuf_len);
      break;
    }
    region->viewport_changed = region->width != region->previous_width ||
                               region->height != region->previous_height;
    if (region->viewport_changed &&
        !kitty_session_set_viewport(region->session, region->width,
                                    region->height)) {
      ok = false;
      g_strlcpy(errbuf, "libkitty rejected a split viewport", errbuf_len);
      break;
    }
    int gl_y = state.viewport[3] - region->y - region->height;
    glViewport(region->x, gl_y, region->width, region->height);
    glEnable(GL_SCISSOR_TEST);
    glScissor(region->x, gl_y, region->width, region->height);
    glClear(GL_COLOR_BUFFER_BIT);
    if (!kitty_session_draw(region->session)) {
      ok = false;
      g_strlcpy(errbuf, "libkitty failed to draw a split", errbuf_len);
      break;
    }
  }
  restore_gl_state(&state);
  return ok;
}

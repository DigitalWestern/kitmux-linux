#include <epoxy/gl.h>
#include <gio/gio.h>
#include <glib-unix.h>
#include <gtk/gtk.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>

#include "gtk_key_translation.h"
#include "libkitty.h"

typedef struct {
  GtkWidget *window;
  GtkWidget *gl_area;
  GtkWidget *status_label;
  GtkWidget *error_label;
  GtkWidget *adjacent_entry;
  kitty_engine *engine;
  kitty_session *session;
  kitmux_key_tracker keys;
  guint pty_source;
  int framebuffer_width;
  int framebuffer_height;
  bool first_frame_reported;
  bool pty_output_reported;
  bool gl_state_reported;
  bool layout_reported;
  bool close_on_child_exit;
} AppState;

typedef struct {
  GLint program;
  GLint vertex_array;
  GLint array_buffer;
  GLint element_array_buffer;
  GLint draw_framebuffer;
  GLint read_framebuffer;
  GLint renderbuffer;
  GLint active_texture;
  GLint texture_2d[4];
  GLint viewport[4];
  GLint scissor_box[4];
  GLint blend_src_rgb;
  GLint blend_dst_rgb;
  GLint blend_src_alpha;
  GLint blend_dst_alpha;
  GLint blend_equation_rgb;
  GLint blend_equation_alpha;
  GLboolean blend;
  GLboolean scissor;
  GLboolean depth_test;
  GLboolean stencil_test;
  GLboolean cull_face;
  GLboolean depth_mask;
  GLboolean color_mask[4];
} GLState;

static void capture_gl_state(GLState *state) {
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
  if (enabled) {
    glEnable(capability);
  } else {
    glDisable(capability);
  }
}

static void restore_gl_state(const GLState *state) {
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

static void show_error(AppState *state, const char *format, ...) {
  char message[1024];
  va_list arguments;
  va_start(arguments, format);
  vsnprintf(message, sizeof(message), format, arguments);
  va_end(arguments);
  gtk_label_set_text(GTK_LABEL(state->error_label), message);
  gtk_widget_set_visible(state->error_label, TRUE);
  gtk_label_set_text(GTK_LABEL(state->status_label), "Terminal unavailable");
  fprintf(stderr, "%s\n", message);
}

static void on_damage(void *userdata) {
  AppState *state = userdata;
  gtk_gl_area_queue_render(GTK_GL_AREA(state->gl_area));
}

static void on_title(void *userdata, const char *title) {
  AppState *state = userdata;
  if (title && *title) {
    gtk_window_set_title(GTK_WINDOW(state->window), title);
  }
}

static void on_bell(void *userdata) {
  AppState *state = userdata;
  gtk_widget_error_bell(state->gl_area);
}

static void on_child_exit(void *userdata, int status) {
  AppState *state = userdata;
  char message[96];
  snprintf(message, sizeof(message), "Shell exited with status %d", status);
  gtk_label_set_text(GTK_LABEL(state->status_label), message);
  printf("GTK terminal child exit: status=%d\n", status);
  fflush(stdout);
  if (state->close_on_child_exit && state->window) {
    gtk_window_close(GTK_WINDOW(state->window));
  }
}

static const char *action_name(int action) {
  switch (action) {
    case KITMUX_KEY_ACTION_RELEASE: return "release";
    case KITMUX_KEY_ACTION_REPEAT: return "repeat";
    default: return "press";
  }
}

// Every key the focused terminal sees is reported here: the translated event
// metadata plus the exact bytes libkitty produced for it. Keys that encode to
// nothing (a release under the legacy protocol) are still reported, so the
// harness can tell "no bytes" from "no event".
static void route_key_to_terminal(AppState *state,
                                  const kitmux_gdk_key_input *input) {
  kitmux_key_translation translated;
  if (!kitmux_translate_gdk_key(input, &translated)) {
    printf("GTK key ignored: action=%s keyval=0x%X\n",
           action_name(input->action), input->keyval);
    fflush(stdout);
    return;
  }
  char encoded[256];
  size_t written = 0;
  if (state->session) {
    written = kitty_session_encode_key(state->session, &translated.event,
                                       encoded, sizeof(encoded));
  }
  char hex[2 * sizeof(encoded) + 1];
  size_t used = 0;
  for (size_t i = 0; i < written && used + 2 < sizeof(hex); ++i) {
    used += (size_t)snprintf(hex + used, sizeof(hex) - used, "%02x",
                             (unsigned char)encoded[i]);
  }
  hex[used] = '\0';
  printf("GTK key %s: key=0x%X shifted=0x%X mods=0x%X text=\"%s\" bytes=%s\n",
         action_name(translated.event.action), translated.event.key,
         translated.event.shifted_key, translated.event.mods,
         translated.event.text, hex);
  fflush(stdout);
  if (written > 0 && state->session) {
    kitty_session_write(state->session, (const uint8_t *)encoded, written);
  }
}

// GDK reports the keyval already resolved for the active layout and level;
// kitty wants the base-layout value as the key identity, so ask GDK for the
// unmodified keyval of the same hardware key in the event's own layout.
static guint base_layout_keyval(GtkWidget *widget,
                                GtkEventController *controller,
                                guint keycode) {
  GdkDisplay *display = gtk_widget_get_display(widget);
  if (!display) return 0;
  GdkEvent *event = gtk_event_controller_get_current_event(controller);
  int group = event ? gdk_key_event_get_layout(event) : 0;
  guint keyval = 0;
  if (!gdk_display_translate_key(display, keycode, GDK_NO_MODIFIER_MASK, group,
                                 &keyval, NULL, NULL, NULL)) {
    return 0;
  }
  return keyval;
}

static gboolean key_pressed(GtkEventControllerKey *controller, guint keyval,
                            guint keycode, GdkModifierType state,
                            gpointer userdata) {
  AppState *app = userdata;
  kitmux_gdk_key_input input = {
      .keyval = keyval,
      .unshifted_keyval = base_layout_keyval(
          app->gl_area, GTK_EVENT_CONTROLLER(controller), keycode),
      .state = state,
      .action = kitmux_key_tracker_press(&app->keys, keycode),
  };
  route_key_to_terminal(app, &input);
  // The terminal owns every key while it is focused, including Tab: GTK must
  // not turn it into focus navigation.
  return TRUE;
}

static void key_released(GtkEventControllerKey *controller, guint keyval,
                         guint keycode, GdkModifierType state,
                         gpointer userdata) {
  AppState *app = userdata;
  kitmux_key_tracker_release(&app->keys, keycode);
  kitmux_gdk_key_input input = {
      .keyval = keyval,
      .unshifted_keyval = base_layout_keyval(
          app->gl_area, GTK_EVENT_CONTROLLER(controller), keycode),
      .state = state,
      .action = KITMUX_KEY_ACTION_RELEASE,
  };
  route_key_to_terminal(app, &input);
}

static void terminal_focus_entered(GtkEventControllerFocus *controller,
                                   gpointer userdata) {
  (void)controller;
  (void)userdata;
  printf("GTK focus: terminal\n");
  fflush(stdout);
}

static void terminal_focus_left(GtkEventControllerFocus *controller,
                                gpointer userdata) {
  (void)controller;
  AppState *state = userdata;
  // Keys held when focus moves away never produce a release the terminal can
  // see, so the held-key set ends with the focus.
  size_t held = kitmux_key_tracker_held(&state->keys);
  kitmux_key_tracker_reset(&state->keys);
  printf("GTK focus: terminal released %zu held key(s)\n", held);
  fflush(stdout);
}

static void adjacent_focus_entered(GtkEventControllerFocus *controller,
                                   gpointer userdata) {
  (void)controller;
  (void)userdata;
  printf("GTK focus: adjacent-control\n");
  fflush(stdout);
}

static void adjacent_text_changed(GtkEditable *editable, gpointer userdata) {
  (void)userdata;
  printf("GTK adjacent control text: %s\n", gtk_editable_get_text(editable));
  fflush(stdout);
}

static void terminal_clicked(GtkGestureClick *gesture, int n_press, double x,
                             double y, gpointer userdata) {
  (void)gesture;
  (void)n_press;
  (void)x;
  (void)y;
  AppState *state = userdata;
  gtk_widget_grab_focus(state->gl_area);
}

// Widget geometry inside the window, so an automated run can put the pointer
// on the terminal or on the ordinary GTK control without guessing.
static void report_layout(AppState *state) {
  struct {
    const char *label;
    GtkWidget *widget;
  } items[] = {
      {"terminal", state->gl_area},
      {"adjacent-control", state->adjacent_entry},
  };
  for (size_t i = 0; i < G_N_ELEMENTS(items); ++i) {
    graphene_rect_t bounds;
    if (!gtk_widget_compute_bounds(items[i].widget, state->window, &bounds)) {
      continue;
    }
    printf("GTK bounds %s: x=%d y=%d w=%d h=%d\n", items[i].label,
           (int)bounds.origin.x, (int)bounds.origin.y, (int)bounds.size.width,
           (int)bounds.size.height);
  }
  fflush(stdout);
}

static gboolean pump_pty(gint fd, GIOCondition condition, gpointer userdata) {
  (void)fd;
  AppState *state = userdata;
  if (!state->session) return G_SOURCE_REMOVE;
  bool changed = kitty_session_pump(state->session);
  size_t pumped = kitty_session_last_pump_bytes(state->session);
  if (pumped > 0 && !state->pty_output_reported) {
    state->pty_output_reported = true;
    printf("GTK terminal PTY output: %zu bytes\n", pumped);
    fflush(stdout);
  }
  if (changed) gtk_gl_area_queue_render(GTK_GL_AREA(state->gl_area));
  if ((condition & (G_IO_HUP | G_IO_ERR | G_IO_NVAL)) != 0 &&
      !kitty_session_child_alive(state->session)) {
    state->pty_source = 0;
    return G_SOURCE_REMOVE;
  }
  return G_SOURCE_CONTINUE;
}

static bool required_environment(AppState *state, kitty_engine_config *config) {
  config->kitty_src_path = g_getenv("KITTY_SRC");
  config->libkitty_py_path = g_getenv("LIBKITTY_PY");
  config->python_home = g_getenv("PYTHONHOME");
  config->config_path = g_getenv("LIBKITTY_TEST_CONFIG");
  if (!config->kitty_src_path || !config->libkitty_py_path ||
      !config->python_home) {
    show_error(state,
               "Missing runtime paths. Set KITTY_SRC, LIBKITTY_PY, and "
               "PYTHONHOME before launching the GTK host.");
    return false;
  }
  return true;
}

static void initialize_terminal(GtkGLArea *area, AppState *state) {
  gtk_gl_area_make_current(area);
  const GError *gl_error = gtk_gl_area_get_error(area);
  if (gl_error) {
    show_error(state, "OpenGL context creation failed: %s", gl_error->message);
    return;
  }

  kitty_engine_config config = {0};
  if (!required_environment(state, &config)) return;
  char error[1024] = {0};
  state->engine = kitty_engine_init(&config, error, sizeof(error));
  if (!state->engine) {
    show_error(state, "libkitty engine initialization failed: %s", error);
    return;
  }

  int cell_width = 0;
  int cell_height = 0;
  double scale = (double)gtk_widget_get_scale_factor(GTK_WIDGET(area));
  if (!kitty_render_init(state->engine, scale, &cell_width, &cell_height,
                         error, sizeof(error))) {
    show_error(state, "libkitty renderer initialization failed: %s", error);
    return;
  }

  kitty_session_callbacks callbacks = {
      .userdata = state,
      .on_damage = on_damage,
      .on_title = on_title,
      .on_bell = on_bell,
      .on_child_exit = on_child_exit,
  };
  // An automated run replaces the login shell with a fixture that records the
  // exact bytes it receives; the child inherits this process's environment.
  const char *child = g_getenv("KITMUX_GTK_CHILD");
  const char *const child_argv[] = {child, NULL};
  state->session = kitty_session_create(state->engine, 24, 80,
                                        (child && *child) ? child_argv : NULL,
                                        &callbacks, error, sizeof(error));
  if (!state->session) {
    show_error(state, "Terminal session creation failed: %s", error);
    return;
  }
  gtk_widget_grab_focus(GTK_WIDGET(area));
  int fd = kitty_session_fd(state->session);
  state->pty_source = g_unix_fd_add_full(
      G_PRIORITY_DEFAULT, fd, G_IO_IN | G_IO_HUP | G_IO_ERR | G_IO_NVAL,
      pump_pty, state, NULL);
  char status[128];
  snprintf(status, sizeof(status), "Live shell · cell %d×%d px", cell_width,
           cell_height);
  gtk_label_set_text(GTK_LABEL(state->status_label), status);
  gtk_widget_set_visible(state->error_label, FALSE);
  printf("GTK terminal ready: cell=%dx%d fd=%d\n", cell_width, cell_height,
         fd);
  fflush(stdout);
}

static void teardown_terminal(GtkGLArea *area, AppState *state) {
  if (state->pty_source) {
    g_source_remove(state->pty_source);
    state->pty_source = 0;
  }
  if (state->session || state->engine) {
    gtk_gl_area_make_current(area);
  }
  if (state->session) {
    kitty_session_close(state->session);
    state->session = NULL;
  }
  if (state->engine) {
    kitty_engine_shutdown(state->engine);
    state->engine = NULL;
  }
}

static void realized(GtkGLArea *area, gpointer userdata) {
  initialize_terminal(area, userdata);
}

static void unrealized(GtkGLArea *area, gpointer userdata) {
  teardown_terminal(area, userdata);
}

static gboolean render(GtkGLArea *area, GdkGLContext *context,
                       gpointer userdata) {
  (void)context;
  AppState *state = userdata;
  if (!state->session) return TRUE;

  int scale = gtk_widget_get_scale_factor(GTK_WIDGET(area));
  int width = gtk_widget_get_width(GTK_WIDGET(area)) * scale;
  int height = gtk_widget_get_height(GTK_WIDGET(area)) * scale;
  if (width <= 0 || height <= 0) return TRUE;

  GLState saved = {0};
  capture_gl_state(&saved);
  glViewport(0, 0, width, height);
  glDisable(GL_SCISSOR_TEST);
  glClearColor(0.04f, 0.05f, 0.07f, 1.0f);
  glClear(GL_COLOR_BUFFER_BIT);

  if (width != state->framebuffer_width ||
      height != state->framebuffer_height) {
    if (!kitty_session_set_viewport(state->session, width, height)) {
      restore_gl_state(&saved);
      show_error(state, "libkitty rejected the %d×%d framebuffer.", width,
                 height);
      return TRUE;
    }
    state->framebuffer_width = width;
    state->framebuffer_height = height;
    printf("GTK terminal viewport: %dx%d scale=%d\n", width, height, scale);
    fflush(stdout);
  }
  bool drawn = kitty_session_draw(state->session);
  restore_gl_state(&saved);
  GLState restored = {0};
  capture_gl_state(&restored);
  if (memcmp(&saved, &restored, sizeof(saved)) != 0) {
    show_error(state, "libkitty did not restore the tracked host OpenGL state.");
    return TRUE;
  }
  if (!drawn) {
    show_error(state, "libkitty failed to draw the terminal frame.");
    return TRUE;
  }
  if (!state->gl_state_reported) {
    state->gl_state_reported = true;
    printf("GTK terminal GL state restoration: OK\n");
    fflush(stdout);
  }
  if (!state->first_frame_reported) {
    state->first_frame_reported = true;
    printf("GTK terminal frame: %dx%d scale=%d pid=%d\n", width, height, scale,
           kitty_session_child_pid(state->session));
    fflush(stdout);
  }
  if (!state->layout_reported) {
    state->layout_reported = true;
    report_layout(state);
  }
  return TRUE;
}

static void close_clicked(GtkButton *button, gpointer userdata) {
  (void)button;
  AppState *state = userdata;
  gtk_window_close(GTK_WINDOW(state->window));
}

static gboolean auto_close(gpointer userdata) {
  AppState *state = userdata;
  if (state->window) gtk_window_close(GTK_WINDOW(state->window));
  return G_SOURCE_REMOVE;
}

static void activate(GtkApplication *application, gpointer userdata) {
  AppState *state = userdata;
  if (state->window) {
    gtk_window_present(GTK_WINDOW(state->window));
    return;
  }
  state->window = gtk_application_window_new(application);
  gtk_window_set_title(GTK_WINDOW(state->window), "Kitmux GTK Terminal Host");
  gtk_window_set_default_size(GTK_WINDOW(state->window), 900, 580);

  GtkWidget *root = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
  GtkWidget *header = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 12);
  gtk_widget_set_margin_start(header, 12);
  gtk_widget_set_margin_end(header, 12);
  gtk_widget_set_margin_top(header, 8);
  gtk_widget_set_margin_bottom(header, 8);
  state->status_label = gtk_label_new("Initializing terminal…");
  gtk_label_set_xalign(GTK_LABEL(state->status_label), 0.0f);
  gtk_widget_set_hexpand(state->status_label, TRUE);
  gtk_box_append(GTK_BOX(header), state->status_label);
  // An ordinary GTK control beside the terminal: focus transfer and key
  // ownership stay observable for the whole spike.
  state->adjacent_entry = gtk_entry_new();
  gtk_entry_set_placeholder_text(GTK_ENTRY(state->adjacent_entry),
                                 "Adjacent GTK entry");
  gtk_widget_set_size_request(state->adjacent_entry, 220, -1);
  g_signal_connect(state->adjacent_entry, "changed",
                   G_CALLBACK(adjacent_text_changed), state);
  GtkEventController *adjacent_focus = gtk_event_controller_focus_new();
  g_signal_connect(adjacent_focus, "enter",
                   G_CALLBACK(adjacent_focus_entered), state);
  gtk_widget_add_controller(state->adjacent_entry, adjacent_focus);
  gtk_box_append(GTK_BOX(header), state->adjacent_entry);
  GtkWidget *close_button = gtk_button_new_with_label("Close");
  g_signal_connect(close_button, "clicked", G_CALLBACK(close_clicked), state);
  gtk_box_append(GTK_BOX(header), close_button);
  gtk_box_append(GTK_BOX(root), header);

  GtkWidget *overlay = gtk_overlay_new();
  state->gl_area = gtk_gl_area_new();
  gtk_gl_area_set_allowed_apis(GTK_GL_AREA(state->gl_area), GDK_GL_API_GL);
  gtk_gl_area_set_required_version(GTK_GL_AREA(state->gl_area), 3, 3);
  gtk_gl_area_set_has_depth_buffer(GTK_GL_AREA(state->gl_area), FALSE);
  gtk_gl_area_set_has_stencil_buffer(GTK_GL_AREA(state->gl_area), FALSE);
  gtk_widget_set_hexpand(state->gl_area, TRUE);
  gtk_widget_set_vexpand(state->gl_area, TRUE);
  gtk_widget_set_focusable(state->gl_area, TRUE);

  GtkEventController *key_controller = gtk_event_controller_key_new();
  g_signal_connect(key_controller, "key-pressed", G_CALLBACK(key_pressed),
                   state);
  g_signal_connect(key_controller, "key-released", G_CALLBACK(key_released),
                   state);
  gtk_widget_add_controller(state->gl_area, key_controller);

  GtkEventController *terminal_focus = gtk_event_controller_focus_new();
  g_signal_connect(terminal_focus, "enter",
                   G_CALLBACK(terminal_focus_entered), state);
  g_signal_connect(terminal_focus, "leave", G_CALLBACK(terminal_focus_left),
                   state);
  gtk_widget_add_controller(state->gl_area, terminal_focus);

  GtkGesture *click = gtk_gesture_click_new();
  g_signal_connect(click, "pressed", G_CALLBACK(terminal_clicked), state);
  gtk_widget_add_controller(state->gl_area, GTK_EVENT_CONTROLLER(click));

  g_signal_connect(state->gl_area, "realize", G_CALLBACK(realized), state);
  g_signal_connect(state->gl_area, "unrealize", G_CALLBACK(unrealized), state);
  g_signal_connect(state->gl_area, "render", G_CALLBACK(render), state);
  gtk_overlay_set_child(GTK_OVERLAY(overlay), state->gl_area);

  state->error_label = gtk_label_new(NULL);
  gtk_label_set_wrap(GTK_LABEL(state->error_label), TRUE);
  gtk_widget_set_halign(state->error_label, GTK_ALIGN_CENTER);
  gtk_widget_set_valign(state->error_label, GTK_ALIGN_CENTER);
  gtk_widget_set_margin_start(state->error_label, 32);
  gtk_widget_set_margin_end(state->error_label, 32);
  gtk_widget_add_css_class(state->error_label, "error");
  gtk_widget_set_visible(state->error_label, FALSE);
  gtk_overlay_add_overlay(GTK_OVERLAY(overlay), state->error_label);
  gtk_box_append(GTK_BOX(root), overlay);
  gtk_window_set_child(GTK_WINDOW(state->window), root);
  gtk_window_present(GTK_WINDOW(state->window));
  // The terminal, not the adjacent entry, owns the keyboard when the window
  // opens; GTK would otherwise focus the first focusable child.
  gtk_window_set_focus(GTK_WINDOW(state->window), state->gl_area);

  const char *close_on_exit = g_getenv("KITMUX_GTK_CLOSE_ON_CHILD_EXIT");
  state->close_on_child_exit = close_on_exit && *close_on_exit &&
                               g_strcmp0(close_on_exit, "0") != 0;

  const char *auto_close_ms = g_getenv("KITMUX_GTK_AUTO_CLOSE_MS");
  if (auto_close_ms && *auto_close_ms) {
    guint64 milliseconds = g_ascii_strtoull(auto_close_ms, NULL, 10);
    if (milliseconds > 0 && milliseconds <= G_MAXUINT) {
      g_timeout_add((guint)milliseconds, auto_close, state);
    }
  }
}

int main(int argc, char **argv) {
  AppState state = {0};
  GtkApplication *application = gtk_application_new(
      "dev.kitmux.gtk-terminal-host", G_APPLICATION_DEFAULT_FLAGS);
  g_signal_connect(application, "activate", G_CALLBACK(activate), &state);
  int status = g_application_run(G_APPLICATION(application), argc, argv);
  g_object_unref(application);
  return status;
}

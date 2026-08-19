#ifndef KITMUX_GTK_TERMINAL_BRIDGE_H
#define KITMUX_GTK_TERMINAL_BRIDGE_H

#include <gtk/gtk.h>
#include <stdbool.h>
#include <stdint.h>

#include "libkitty.h"

GtkWidget *kitmux_product_terminal_area_new(void);
uint32_t kitmux_gdk_base_layout_keyval(GtkWidget *widget,
                                       GtkEventController *controller,
                                       uint32_t keycode);
typedef struct {
  int framebuffer_width;
  int framebuffer_height;
  int cell_width;
  int cell_height;
  double render_scale;
  bool metrics_changed;
  bool viewport_changed;
} kitmux_render_result;

bool kitmux_terminal_render_frame(kitty_engine *engine, kitty_session *session,
                                  double buffer_scale,
                                  int previous_width, int previous_height,
                                  kitmux_render_result *result,
                                  char *errbuf, size_t errbuf_len);

typedef struct {
  kitty_session *session;
  int x;
  int y;
  int width;
  int height;
  int previous_width;
  int previous_height;
  bool viewport_changed;
} kitmux_terminal_region;

bool kitmux_terminal_render_regions(kitty_engine *engine, double buffer_scale,
                                    kitmux_terminal_region *regions,
                                    size_t region_count, int *cell_width,
                                    int *cell_height, char *errbuf,
                                    size_t errbuf_len);

#endif

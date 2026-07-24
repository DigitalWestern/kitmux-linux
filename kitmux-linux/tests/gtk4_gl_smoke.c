#include <epoxy/gl.h>
#include <gtk/gtk.h>

static gboolean render(GtkGLArea *area, GdkGLContext *context, gpointer data) {
  (void)area;
  (void)context;
  (void)data;

  glClearColor(0.08f, 0.10f, 0.16f, 1.0f);
  glClear(GL_COLOR_BUFFER_BIT);
  return TRUE;
}

static void activate(GtkApplication *application, gpointer data) {
  (void)data;

  GtkWidget *window = gtk_application_window_new(application);
  gtk_window_set_title(GTK_WINDOW(window), "Kitmux GTK 4 OpenGL Proof");
  gtk_window_set_default_size(GTK_WINDOW(window), 720, 480);

  GtkWidget *overlay = gtk_overlay_new();
  GtkWidget *gl_area = gtk_gl_area_new();
  gtk_gl_area_set_required_version(GTK_GL_AREA(gl_area), 3, 3);
  g_signal_connect(gl_area, "render", G_CALLBACK(render), NULL);
  gtk_widget_set_hexpand(gl_area, TRUE);
  gtk_widget_set_vexpand(gl_area, TRUE);
  gtk_overlay_set_child(GTK_OVERLAY(overlay), gl_area);

  GtkWidget *label = gtk_label_new(
      "Kitmux Linux desktop VM\nGTK 4 + GtkGLArea smoke");
  gtk_widget_set_halign(label, GTK_ALIGN_CENTER);
  gtk_widget_set_valign(label, GTK_ALIGN_CENTER);
  gtk_widget_add_css_class(label, "title-1");
  gtk_overlay_add_overlay(GTK_OVERLAY(overlay), label);

  gtk_window_set_child(GTK_WINDOW(window), overlay);
  gtk_window_present(GTK_WINDOW(window));
}

int main(int argc, char **argv) {
  GtkApplication *application =
      gtk_application_new("dev.kitmux.gtk4-gl-smoke",
                          G_APPLICATION_DEFAULT_FLAGS);
  g_signal_connect(application, "activate", G_CALLBACK(activate), NULL);
  int status = g_application_run(G_APPLICATION(application), argc, argv);
  g_object_unref(application);
  return status;
}

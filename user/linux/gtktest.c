/* gtktest — um programa GTK 3 de verdade no Pulsar (backend Wayland):
 * rotulo com markup, campo de texto, botoes, caixa de selecao, controle
 * deslizante, barra de progresso animada e lista suspensa (que abre um
 * popup xdg_popup). Cada evento e impresso, para os testes dirigirem a
 * janela por cliques e teclas.
 *
 *   gtktest        fica aberto ~40 s; termina com "fim: ok" se a janela foi
 *                  desenhada, o botao recebeu clique e o campo recebeu texto
 *   gtktest loop   fica aberto ate fechar a janela */
#include <gtk/gtk.h>
#include <stdio.h>
#include <string.h>

static int draws, clicks, loop_mode;
static GtkWidget *count_label, *entry, *progress, *combo;
static char typed[256];

static gboolean on_draw(GtkWidget* w, cairo_t* cr, gpointer d) {
    if (draws++ == 0) printf("[gtktest] janela desenhada (%dx%d)\n", gtk_widget_get_allocated_width(w),
                             gtk_widget_get_allocated_height(w));
    return FALSE;
}
static void on_click(GtkButton* b, gpointer d) {
    clicks++;
    char s[64];
    snprintf(s, sizeof s, "Cliques: %d", clicks);
    gtk_label_set_text(GTK_LABEL(count_label), s);
    printf("[gtktest] botao clicado (%d)\n", clicks);
}
static void on_changed(GtkEditable* e, gpointer d) {
    snprintf(typed, sizeof typed, "%s", gtk_entry_get_text(GTK_ENTRY(entry)));
}
static void on_activate(GtkEntry* e, gpointer d) { printf("[gtktest] Enter no campo: \"%s\"\n", typed); }
static void on_toggle(GtkToggleButton* t, gpointer d) {
    printf("[gtktest] caixa de selecao: %s\n", gtk_toggle_button_get_active(t) ? "marcada" : "desmarcada");
}
static void on_combo(GtkComboBox* c, gpointer d) {
    char* t = gtk_combo_box_text_get_active_text(GTK_COMBO_BOX_TEXT(c));
    printf("[gtktest] lista suspensa: %s\n", t ? t : "(nada)");
    g_free(t);
}
static void on_popup_shown(GObject* o, GParamSpec* p, gpointer d) {
    gboolean shown;
    g_object_get(o, "popup-shown", &shown, NULL);
    printf("[gtktest] popup da lista %s\n", shown ? "aberto" : "fechado");
}
static gboolean pulse(gpointer d) {
    gtk_progress_bar_pulse(GTK_PROGRESS_BAR(progress));
    return G_SOURCE_CONTINUE;
}
static gboolean finish(gpointer win) {
    int ok = draws > 0 && clicks > 0 && typed[0];
    printf("[gtktest] desenhos %d, cliques %d, texto \"%s\"\n", draws, clicks, typed);
    printf("[gtktest] fim: %s\n", ok ? "ok" : "FALHA");
    gtk_main_quit();
    return G_SOURCE_REMOVE;
}

int main(int argc, char** argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    loop_mode = argc > 1 && !strcmp(argv[1], "loop");
    /* no boot (autorun) o servico Wayland pode ainda nao ter subido, e o GTK
     * nao tenta de novo: espera o socket */
    const char* rt = g_getenv("XDG_RUNTIME_DIR");
    char* sock = g_build_filename(rt ? rt : "/run", g_getenv("WAYLAND_DISPLAY") ? g_getenv("WAYLAND_DISPLAY") : "wayland-0", NULL);
    for (int i = 0; i < 100 && !g_file_test(sock, G_FILE_TEST_EXISTS); i++) g_usleep(100000);
    g_free(sock);
    gint64 t0 = g_get_monotonic_time();
    if (!gtk_init_check(NULL, NULL)) { printf("[gtktest] gtk_init falhou\n[gtktest] fim: FALHA\n"); return 1; }
    printf("[gtktest] GTK %d.%d.%d iniciado em %lld ms (backend %s)\n", gtk_get_major_version(),
           gtk_get_minor_version(), gtk_get_micro_version(), (long long)(g_get_monotonic_time() - t0) / 1000,
           G_OBJECT_TYPE_NAME(gdk_display_get_default()));

    GtkWidget* win = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(win), "GTK 3 no Pulsar");
    gtk_window_set_default_size(GTK_WINDOW(win), 460, 380);
    g_signal_connect(win, "destroy", G_CALLBACK(gtk_main_quit), NULL);
    g_signal_connect(win, "draw", G_CALLBACK(on_draw), NULL);

    GtkWidget* box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 10);
    gtk_container_set_border_width(GTK_CONTAINER(box), 16);
    gtk_container_add(GTK_CONTAINER(win), box);

    GtkWidget* title = gtk_label_new(NULL);
    gtk_label_set_markup(GTK_LABEL(title), "<span size='x-large' weight='bold'>Olá do GTK 3!</span>\n"
                                           "<small>pango, cairo, glib e Wayland por baixo</small>");
    gtk_box_pack_start(GTK_BOX(box), title, FALSE, FALSE, 0);

    entry = gtk_entry_new();
    gtk_entry_set_placeholder_text(GTK_ENTRY(entry), "Digite algo e tecle Enter");
    g_signal_connect(entry, "changed", G_CALLBACK(on_changed), NULL);
    g_signal_connect(entry, "activate", G_CALLBACK(on_activate), NULL);
    gtk_box_pack_start(GTK_BOX(box), entry, FALSE, FALSE, 0);

    GtkWidget* row = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 10);
    GtkWidget* button = gtk_button_new_with_label("Clique aqui");
    g_signal_connect(button, "clicked", G_CALLBACK(on_click), NULL);
    count_label = gtk_label_new("Cliques: 0");
    gtk_box_pack_start(GTK_BOX(row), button, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(row), count_label, FALSE, FALSE, 0);
    gtk_box_pack_start(GTK_BOX(box), row, FALSE, FALSE, 0);

    GtkWidget* check = gtk_check_button_new_with_label("Caixa de seleção");
    g_signal_connect(check, "toggled", G_CALLBACK(on_toggle), NULL);
    gtk_box_pack_start(GTK_BOX(box), check, FALSE, FALSE, 0);

    GtkWidget* scale = gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 0, 100, 1);
    gtk_range_set_value(GTK_RANGE(scale), 40);
    gtk_box_pack_start(GTK_BOX(box), scale, FALSE, FALSE, 0);

    progress = gtk_progress_bar_new();
    gtk_box_pack_start(GTK_BOX(box), progress, FALSE, FALSE, 0);
    g_timeout_add(120, pulse, NULL);

    combo = gtk_combo_box_text_new();
    const char* items[] = { "Pulsar OS", "Linux (compatibilidade)", "Wayland", "GTK 3" };
    for (int i = 0; i < 4; i++) gtk_combo_box_text_append_text(GTK_COMBO_BOX_TEXT(combo), items[i]);
    gtk_combo_box_set_active(GTK_COMBO_BOX(combo), 0);
    g_signal_connect(combo, "changed", G_CALLBACK(on_combo), NULL);
    g_signal_connect(combo, "notify::popup-shown", G_CALLBACK(on_popup_shown), NULL);
    gtk_box_pack_start(GTK_BOX(box), combo, FALSE, FALSE, 0);

    gtk_widget_show_all(win);
    if (!loop_mode) g_timeout_add_seconds(40, finish, win);
    gtk_main();
    return 0;
}

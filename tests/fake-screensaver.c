/* Stand-in for phosh's org.gnome.ScreenSaver for tests/daemon.sh. Test.Control
   changes Active or drops the bus name. */

#include <gio/gio.h>

static const gchar xml[] =
    "<node>"
    " <interface name='org.gnome.ScreenSaver'>"
    "  <method name='GetActive'><arg type='b' direction='out'/></method>"
    "  <signal name='ActiveChanged'><arg type='b'/></signal>"
    " </interface>"
    " <interface name='test.Control'>"
    "  <method name='SetActive'><arg type='b' direction='in'/></method>"
    "  <method name='ReleaseName'/>"
    " </interface>"
    "</node>";

static gboolean active;
static guint owner_id;

static void
handle_call(GDBusConnection *conn, const gchar *sender, const gchar *path,
            const gchar *iface, const gchar *method, GVariant *params,
            GDBusMethodInvocation *inv, gpointer data)
{
    (void)sender; (void)iface; (void)data;

    if (g_strcmp0(method, "GetActive") == 0) {
        g_dbus_method_invocation_return_value(inv, g_variant_new("(b)", active));
    } else if (g_strcmp0(method, "SetActive") == 0) {
        g_variant_get(params, "(b)", &active);
        g_dbus_connection_emit_signal(conn, NULL, path, "org.gnome.ScreenSaver",
                                      "ActiveChanged", g_variant_new("(b)", active), NULL);
        g_dbus_method_invocation_return_value(inv, NULL);
    } else if (g_strcmp0(method, "ReleaseName") == 0) {
        g_dbus_method_invocation_return_value(inv, NULL);
        g_bus_unown_name(owner_id);
        owner_id = 0;
    }
}

static const GDBusInterfaceVTable vtable = { handle_call, NULL, NULL, { 0 } };

int
main(int argc, char **argv)
{
    g_autoptr(GError) error = NULL;
    g_autoptr(GDBusNodeInfo) info = g_dbus_node_info_new_for_xml(xml, &error);
    GDBusConnection *conn = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &error);

    active = argc > 1 && g_strcmp0(argv[1], "active") == 0;
    if (!conn || !info)
        g_error("setup failed: %s", error->message);

    for (guint i = 0; info->interfaces[i]; i++)
        g_dbus_connection_register_object(conn, "/org/gnome/ScreenSaver", info->interfaces[i],
                                          &vtable, NULL, NULL, &error);

    owner_id = g_bus_own_name_on_connection(conn, "org.gnome.ScreenSaver",
                                            G_BUS_NAME_OWNER_FLAGS_NONE, NULL, NULL, NULL, NULL);
    g_main_loop_run(g_main_loop_new(NULL, FALSE));
    return 0;
}

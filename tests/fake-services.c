/* Owns the bus names given as arguments, standing in for gsd, feedbackd and
   iio-sensor-proxy in tests/daemon.sh. net.hadess.SensorProxy also gets a
   HasAmbientLight property, false when FAKE_NO_AMBIENT_LIGHT is set. --system
   uses the system bus. */

#include <gio/gio.h>
#include <string.h>

static const gchar sensor_xml[] =
    "<node>"
    " <interface name='net.hadess.SensorProxy'>"
    "  <property name='HasAmbientLight' type='b' access='read'/>"
    " </interface>"
    "</node>";

static GVariant *
get_property(GDBusConnection *conn, const gchar *sender, const gchar *path,
             const gchar *iface, const gchar *name, GError **error, gpointer data)
{
    (void)conn; (void)sender; (void)path; (void)iface; (void)error; (void)data;
    if (g_strcmp0(name, "HasAmbientLight") == 0)
        return g_variant_new_boolean(g_getenv("FAKE_NO_AMBIENT_LIGHT") == NULL);
    return NULL;
}

static const GDBusInterfaceVTable vtable = { NULL, get_property, NULL, { 0 } };

int
main(int argc, char **argv)
{
    g_autoptr(GError) error = NULL;
    GBusType type = G_BUS_TYPE_SESSION;
    int first = 1;

    if (argc > 1 && strcmp(argv[1], "--system") == 0) {
        type = G_BUS_TYPE_SYSTEM;
        first = 2;
    }
    GDBusConnection *conn = g_bus_get_sync(type, NULL, &error);
    if (!conn)
        g_error("no bus: %s", error->message);

    for (int i = first; i < argc; i++) {
        if (strcmp(argv[i], "net.hadess.SensorProxy") == 0) {
            g_autoptr(GDBusNodeInfo) info = g_dbus_node_info_new_for_xml(sensor_xml, &error);
            g_dbus_connection_register_object(conn, "/net/hadess/SensorProxy",
                                              info->interfaces[0], &vtable, NULL, NULL, &error);
        }
        g_bus_own_name_on_connection(conn, argv[i], G_BUS_NAME_OWNER_FLAGS_NONE,
                                     NULL, NULL, NULL, NULL);
    }
    g_main_loop_run(g_main_loop_new(NULL, FALSE));
    return 0;
}

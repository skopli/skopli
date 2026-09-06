using System.Text.Json;

namespace Skopli.Internal;

/// <summary>
/// Unusable-payload validation through the core parser, without reimplementing
/// any source parser facade-side. Builds a throwaway native
/// pricing handle holding exactly the one raw catalog, reads the parsed model
/// count from <c>ag_pricing_catalog_info</c>, and frees the handle. A build
/// error (including invalid-JSON payload), or a zero model count, means the
/// payload is unusable.
/// </summary>
internal static class Probe
{
    /// <summary>
    /// True when the raw catalog contributes at least one price for
    /// <paramref name="sourceName"/> through the core parser. Any error building
    /// the throwaway handle (a malformed payload, an unknown format) is unusable.
    /// </summary>
    internal static bool Usable(string sourceName, Catalog catalog)
    {
        string optsJson;
        try
        {
            // an invalid-JSON payload fails to build the wire options (a
            // JsonException from parsing, or a SkopliException from validation)
            optsJson = WireBuilder.Pricing(PricingMode.Calculate, null, new[] { catalog });
        }
        catch (Exception ex) when (ex is SkopliException or JsonException)
        {
            return false;
        }
        IntPtr handle;
        try
        {
            handle = Interop.PricingNew(optsJson);
        }
        catch (SkopliException)
        {
            return false;
        }
        try
        {
            string json = Interop.PricingCatalogInfo(handle);
            List<CatalogInfo> infos = Json.DeserializeArray<CatalogInfo>(json);
            foreach (CatalogInfo info in infos)
            {
                if (info.Source == sourceName && info.Models > 0)
                {
                    return true;
                }
            }
            return false;
        }
        finally
        {
            Interop.PricingFree(handle);
        }
    }
}

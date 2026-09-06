using System.Runtime.InteropServices;
using System.Text;
using Skopli.Native;

namespace Skopli.Internal;

/// <summary>
/// The safe managed wrapper over the csbindgen P/Invoke layer
/// (<see cref="NativeMethods"/>). Owns the buffer/string lifetime contract of the
/// C ABI: every <c>AgBuf</c> handed out is decoded to a UTF-8 string and freed
/// with <c>ag_buf_free</c>; every library-owned C string is freed with
/// <c>ag_string_free</c>. Each fallible call's <see cref="AgStatus"/> is mapped
/// to the facade exception hierarchy, attaching this thread's last-error detail.
/// </summary>
internal static unsafe class Interop
{
    internal static uint AbiVersion() => NativeMethods.ag_abi_version();

    internal static uint SchemaVersion() => NativeMethods.ag_schema_version();

    internal static string Version()
    {
        byte* ptr = NativeMethods.ag_version();
        return ptr is null ? string.Empty : Marshal.PtrToStringUTF8((IntPtr)ptr) ?? string.Empty;
    }

    internal static string DefaultCacheDir()
    {
        byte* str = null;
        Check(NativeMethods.ag_default_cache_dir(&str), "ag_default_cache_dir");
        return TakeString(str);
    }

    internal static string DetectHarnesses(string? optsJson) =>
        CallJson(optsJson, "ag_detect_harnesses", static (json, len, @out) =>
            NativeMethods.ag_detect_harnesses(json, len, @out));

    internal static string ReadUsage(string? optsJson) =>
        CallJson(optsJson, "ag_read_usage", static (json, len, @out) =>
            NativeMethods.ag_read_usage(json, len, @out));

    internal static string Rollup(string eventsJson, string optsJson)
    {
        AgBuf buf = default;
        byte[] events = Utf8(eventsJson);
        byte[] opts = Utf8(optsJson);
        fixed (byte* e = events)
        fixed (byte* o = opts)
        {
            Check(NativeMethods.ag_rollup(e, (nuint)events.Length, o, (nuint)opts.Length, &buf), "ag_rollup");
        }
        return TakeBuf(buf);
    }

    internal static double CostUsd(string tokensJson, string priceJson)
    {
        double result = 0;
        byte[] tokens = Utf8(tokensJson);
        byte[] price = Utf8(priceJson);
        fixed (byte* t = tokens)
        fixed (byte* p = price)
        {
            Check(NativeMethods.ag_cost_usd(t, (nuint)tokens.Length, p, (nuint)price.Length, &result), "ag_cost_usd");
        }
        return result;
    }

    internal static IntPtr PricingNew(string optsJson)
    {
        AgPricing* handle = null;
        byte[] opts = Utf8(optsJson);
        fixed (byte* o = opts)
        {
            Check(NativeMethods.ag_pricing_new(o, (nuint)opts.Length, &handle), "ag_pricing_new");
        }
        return (IntPtr)handle;
    }

    internal static void PricingFree(IntPtr handle) =>
        NativeMethods.ag_pricing_free((AgPricing*)handle);

    internal static string PricingCatalogInfo(IntPtr handle)
    {
        AgBuf buf = default;
        Check(NativeMethods.ag_pricing_catalog_info((AgPricing*)handle, &buf), "ag_pricing_catalog_info");
        return TakeBuf(buf);
    }

    internal static string PricingPriceEvents(IntPtr handle, string eventsJson, string optsJson)
    {
        AgBuf buf = default;
        byte[] events = Utf8(eventsJson);
        byte[] opts = Utf8(optsJson);
        fixed (byte* e = events)
        fixed (byte* o = opts)
        {
            Check(NativeMethods.ag_pricing_price_events((AgPricing*)handle, e, (nuint)events.Length, o, (nuint)opts.Length, &buf), "ag_pricing_price_events");
        }
        return TakeBuf(buf);
    }

    internal static string PricingPriceRollups(IntPtr handle, string rollupsJson)
    {
        AgBuf buf = default;
        byte[] rollups = Utf8(rollupsJson);
        fixed (byte* r = rollups)
        {
            Check(NativeMethods.ag_pricing_price_rollups((AgPricing*)handle, r, (nuint)rollups.Length, &buf), "ag_pricing_price_rollups");
        }
        return TakeBuf(buf);
    }

    internal static string PricingLookupModel(IntPtr handle, string model)
    {
        AgBuf buf = default;
        byte[] name = Utf8(model);
        fixed (byte* m = name)
        {
            Check(NativeMethods.ag_pricing_lookup_model((AgPricing*)handle, m, (nuint)name.Length, &buf), "ag_pricing_lookup_model");
        }
        return TakeBuf(buf);
    }

    // -----------------------------------------------------------------------

    private delegate AgStatus JsonCall(byte* json, nuint len, AgBuf* @out);

    private static string CallJson(string? optsJson, string op, JsonCall call)
    {
        AgBuf buf = default;
        byte[]? opts = optsJson is null ? null : Utf8(optsJson);
        fixed (byte* o = opts)
        {
            Check(call(o, (nuint)(opts?.Length ?? 0), &buf), op);
        }
        return TakeBuf(buf);
    }

    private static byte[] Utf8(string value) => Encoding.UTF8.GetBytes(value);

    private static string TakeBuf(AgBuf buf)
    {
        if (buf.ptr is null || buf.len == 0)
        {
            return string.Empty;
        }
        try
        {
            return Encoding.UTF8.GetString(buf.ptr, (int)buf.len);
        }
        finally
        {
            NativeMethods.ag_buf_free(buf);
        }
    }

    private static string TakeString(byte* str)
    {
        if (str is null)
        {
            return string.Empty;
        }
        try
        {
            return Marshal.PtrToStringUTF8((IntPtr)str) ?? string.Empty;
        }
        finally
        {
            NativeMethods.ag_string_free(str);
        }
    }

    private static void Check(AgStatus status, string op)
    {
        if (status == AgStatus.Ok)
        {
            return;
        }
        string detail = LastError() ?? $"{op} failed";
        throw status switch
        {
            AgStatus.InvalidArgument => new InvalidArgumentException(detail),
            AgStatus.Catalog => new CatalogException(detail),
            _ => new InternalException(detail),
        };
    }

    private static string? LastError()
    {
        byte* str = null;
        if (NativeMethods.ag_last_error_message(&str) != AgStatus.Ok || str is null)
        {
            return null;
        }
        return TakeString(str);
    }
}

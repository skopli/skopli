namespace Skopli;

/// <summary>
/// Base type for every error surfaced by the skopli facade. Maps the C ABI's
/// <c>AgStatus</c> codes into the .NET idiom: programmer
/// mistakes and I/O-fatal conditions throw; malformed data does not (it rides the
/// diagnostics/skipped channels of <see cref="ReadUsageResult"/>).
/// </summary>
public class SkopliException : Exception
{
    public SkopliException(string message)
        : base(message)
    {
    }

    public SkopliException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}

/// <summary>
/// A caller-supplied argument was invalid (<c>AgStatus.InvalidArgument</c>) - a
/// null-where-required, malformed JSON, or an out-of-range date/timezone. Derives
/// from <see cref="SkopliException"/> and carries <see cref="ArgumentException"/>
/// semantics for bad-argument idiom.
/// </summary>
public sealed class InvalidArgumentException : SkopliException
{
    public InvalidArgumentException(string message)
        : base(message)
    {
    }
}

/// <summary>
/// A pricing/catalog operation failed (<c>AgStatus.Catalog</c>) - e.g. an
/// unparseable catalog payload, or a source's fetch threw facade-side.
/// </summary>
public sealed class CatalogException : SkopliException
{
    public CatalogException(string message)
        : base(message)
    {
    }

    public CatalogException(string message, Exception innerException)
        : base(message, innerException)
    {
    }
}

/// <summary>
/// An unexpected internal failure (<c>AgStatus.Internal</c>), including a caught
/// panic on the native side.
/// </summary>
public sealed class InternalException : SkopliException
{
    public InternalException(string message)
        : base(message)
    {
    }
}

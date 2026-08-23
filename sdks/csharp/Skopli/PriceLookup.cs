namespace Skopli;

/// <summary>
/// The outcome of pricing a group of events: either a <see cref="PriceHit"/>
/// (a model matched a catalog price) or a <see cref="PriceMiss"/> (no match).
/// Discriminated by <see cref="Priced"/>; pattern-match on the derived type.
/// </summary>
public abstract record PriceLookup
{
    /// <summary>True for a <see cref="PriceHit"/>, false for a <see cref="PriceMiss"/>.</summary>
    public abstract bool Priced { get; }
}

/// <summary>A priced group: a model matched a catalog entry and a USD cost was produced.</summary>
public sealed record PriceHit : PriceLookup
{
    public override bool Priced => true;

    /// <summary>The model (or the group's sole model) that was priced.</summary>
    public required string Model { get; init; }

    /// <summary>The catalog key the model matched (may differ from <see cref="Model"/> via aliasing).</summary>
    public string? Key { get; init; }

    /// <summary>The matched price, when the group is a single model.</summary>
    public ModelPrice? Price { get; init; }

    /// <summary>The catalog source name (e.g. openrouter/litellm/override).</summary>
    public string? Source { get; init; }

    /// <summary>When the source catalog was fetched, or null for the override catalog.</summary>
    public string? FetchedAt { get; init; }

    /// <summary>The computed USD cost for the group.</summary>
    public double Usd { get; init; }

    /// <summary>True when a tiered model was aggregated across multiple calls.</summary>
    public bool TieredAggregate { get; init; }

    /// <summary>When a group spans multiple models, the sorted set of models priced.</summary>
    public IReadOnlyList<string>? Models { get; init; }
}

/// <summary>An unpriced group: no catalog matched any of the group's models.</summary>
public sealed record PriceMiss : PriceLookup
{
    public override bool Priced => false;

    /// <summary>The model (or group key) that could not be priced.</summary>
    public required string Model { get; init; }

    /// <summary>The keys that were tried during lookup.</summary>
    public IReadOnlyList<string> Attempted { get; init; } = Array.Empty<string>();

    /// <summary>Why the lookup missed, when known.</summary>
    public string? Reason { get; init; }

    /// <summary>The normalized key that was searched, when known.</summary>
    public string? Key { get; init; }

    /// <summary>The USD attributed under display/auto modes (recorded costs), when present.</summary>
    public double? Usd { get; init; }
}

/// <summary>A rollup bucket paired with the outcome of pricing its events.</summary>
public sealed record PricedRollup
{
    public required Rollup Rollup { get; init; }

    public required PriceLookup Pricing { get; init; }
}

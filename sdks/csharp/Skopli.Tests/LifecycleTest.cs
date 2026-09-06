using System.Text.Json;
using System.Text.Json.Nodes;
using Skopli;
using Xunit;

namespace Skopli.Tests;

/// <summary>
/// Drives the C# built-in pricing lifecycle against the SHARED conformance
/// fixtures in <c>golden/pricing/lifecycle/</c> (the same gold the TypeScript
/// reference and every other facade consume), plus full <c>PriceRollups</c> gold
/// equality and a <c>LookupModel</c> hit + miss. A facade that drifts fails
/// against the shared gold rather than its author's assumptions.
/// </summary>
public sealed class LifecycleTest
{
    private static string RepoRoot()
    {
        string dir = AppContext.BaseDirectory;
        while (dir is not null && !Directory.Exists(Path.Combine(dir, "golden")))
        {
            dir = Path.GetDirectoryName(dir)!;
        }
        Assert.False(dir is null, "could not locate repo root (golden/)");
        return dir!;
    }

    private static string GoldenPath(params string[] parts) =>
        Path.Combine(new[] { RepoRoot(), "golden" }.Concat(parts).ToArray());

    private static JsonObject LoadFixture(params string[] parts) =>
        JsonNode.Parse(File.ReadAllText(GoldenPath(new[] { "pricing", "lifecycle" }.Concat(parts).ToArray())))!
            .AsObject();

    private static readonly JsonObject Request = LoadFixture("request.json");
    private static readonly JsonObject FreshCache = LoadFixture("cache-fresh.json");
    private static readonly JsonObject StaleCache = LoadFixture("cache-stale.json");

    private const string SourceUrl = "https://or.test/models";

    // the canonical OpenRouter source payload the matrix fetches, consumed from
    // source-openrouter.json (the README-named canonical payload)
    private static string SourcePayload() =>
        LoadFixture("source-openrouter.json").ToJsonString();

    private static long TtlMs() => (long)Request["ttlMs"]!;

    private static string CacheFileName() => (string)Request["cacheFileName"]!;

    private static string RequestSource() => (string)Request["source"]!;

    private static PricingMode RequestMode() =>
        (string)Request["options"]!["mode"]! switch
        {
            "calculate" => PricingMode.Calculate,
            "auto" => PricingMode.Auto,
            "display" => PricingMode.Display,
            var m => throw new ArgumentException($"unknown mode {m}"),
        };

    // select the built-in source factory named by request.source
    private static Func<string, PricingSource> SourceFactory(string name) => name switch
    {
        "openrouter" => PricingSource.OpenRouter,
        "litellm" => PricingSource.LiteLlm,
        "models-dev" => PricingSource.ModelsDev,
        _ => throw new ArgumentException($"unknown source {name}"),
    };

    private static Func<DateTimeOffset> FixedClock()
    {
        DateTimeOffset now = DateTimeOffset.Parse(
            (string)Request["now"]!, null, System.Globalization.DateTimeStyles.AdjustToUniversal);
        return () => now;
    }

    private static Rollup RequestRollup()
    {
        JsonObject r = Request["rollup"]!.AsObject();
        return r.Deserialize<Rollup>(new JsonSerializerOptions())!;
    }

    private sealed class RecordingFetch
    {
        private readonly string _payload;
        private readonly bool _throws;

        public RecordingFetch(string payload, bool throws = false)
        {
            _payload = payload;
            _throws = throws;
        }

        public int Calls { get; private set; }

        public Func<string, Task<string>> Fetch => _ =>
        {
            Calls++;
            if (_throws)
            {
                throw new HttpRequestException("network down");
            }
            return Task.FromResult(_payload);
        };
    }

    private static string TempDir()
    {
        string dir = Path.Combine(Path.GetTempPath(), "skopli-csharp-lifecycle-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        return dir;
    }

    private static void InstallCache(string dir, JsonObject cache) =>
        File.WriteAllText(Path.Combine(dir, CacheFileName()), cache.ToJsonString());

    private static string? CurrentFetchedAt(string dir)
    {
        string path = Path.Combine(dir, CacheFileName());
        if (!File.Exists(path))
        {
            return null;
        }
        return (string)JsonNode.Parse(File.ReadAllText(path))!.AsObject()["fetchedAt"]!;
    }

    private static (PricedRollup Priced, RecordingFetch Stub) Run(
        string dir, RecordingFetch stub, bool offline = false, bool refresh = false)
    {
        Func<string, PricingSource> make = SourceFactory(RequestSource());
        var options = new PricingOptions
        {
            Mode = RequestMode(),
            Sources = new[] { make(SourceUrl) },
            CacheDir = dir,
            Fetch = stub.Fetch,
            Clock = FixedClock(),
            TtlMs = TtlMs(),
            Offline = offline,
            Refresh = refresh,
        };
        using Pricing pricing = SkopliClient.CreatePricing(options);
        PricedRollup priced = pricing.PriceRollups(new[] { RequestRollup() }).Single();
        return (priced, stub);
    }

    // -- the five-behavior conformance matrix -------------------------------

    private static JsonObject Expected(string behavior)
    {
        JsonObject exp = LoadFixture("expected", $"{behavior}.json");
        Assert.Equal(behavior, (string)exp["behavior"]!);
        return exp;
    }

    private static void AssertPriced(JsonObject expected, PricedRollup priced)
    {
        bool wantPriced = (bool)expected["priced"]!;
        if (wantPriced)
        {
            PriceHit hit = Assert.IsType<PriceHit>(priced.Pricing);
            Assert.True(hit.Priced);
            Assert.Equal((double)expected["pricedUsd"]!, hit.Usd, 10);
        }
        else
        {
            PriceMiss miss = Assert.IsType<PriceMiss>(priced.Pricing);
            Assert.False(miss.Priced);
            Assert.Equal((double)expected["pricedUsd"]!, miss.Usd ?? 0.0, 10);
        }
    }

    [Fact]
    public void ColdFetch()
    {
        JsonObject exp = Expected("cold-fetch");
        string dir = TempDir();
        try
        {
            var stub = new RecordingFetch(SourcePayload());
            (PricedRollup priced, _) = Run(dir, stub);
            Assert.Equal((bool)exp["fetched"]! ? 1 : 0, stub.Calls);
            AssertPriced(exp, priced);
            Assert.Equal((string?)exp["fetchedAt"], CurrentFetchedAt(dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void WarmCache()
    {
        JsonObject exp = Expected("warm-cache");
        string dir = TempDir();
        try
        {
            InstallCache(dir, FreshCache);
            var stub = new RecordingFetch(SourcePayload());
            (PricedRollup priced, _) = Run(dir, stub);
            Assert.Equal(0, stub.Calls);
            Assert.False((bool)exp["fetched"]!);
            AssertPriced(exp, priced);
            Assert.Equal((string?)exp["fetchedAt"], CurrentFetchedAt(dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void TtlRefresh()
    {
        JsonObject exp = Expected("ttl-refresh");
        string dir = TempDir();
        try
        {
            InstallCache(dir, StaleCache);
            var stub = new RecordingFetch(SourcePayload());
            (PricedRollup priced, _) = Run(dir, stub, refresh: true);
            Assert.Equal(1, stub.Calls);
            Assert.True((bool)exp["fetched"]!);
            AssertPriced(exp, priced);
            Assert.Equal((string?)exp["fetchedAt"], CurrentFetchedAt(dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void Offline()
    {
        JsonObject exp = Expected("offline");
        string dir = TempDir();
        try
        {
            InstallCache(dir, StaleCache);
            var stub = new RecordingFetch(SourcePayload());
            (PricedRollup priced, _) = Run(dir, stub, offline: true);
            Assert.Equal(0, stub.Calls);
            Assert.False((bool)exp["fetched"]!);
            AssertPriced(exp, priced);
            Assert.Equal((string?)exp["fetchedAt"], CurrentFetchedAt(dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void OfflineNoCache()
    {
        JsonObject exp = Expected("offline-no-cache");
        string dir = TempDir();
        try
        {
            var stub = new RecordingFetch(SourcePayload());
            (PricedRollup priced, _) = Run(dir, stub, offline: true);
            Assert.Equal(0, stub.Calls);
            Assert.False((bool)exp["fetched"]!);
            Assert.Null((string?)exp["fetchedAt"]);
            Assert.False(File.Exists(Path.Combine(dir, CacheFileName())));
            AssertPriced(exp, priced);
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void StaleFallback()
    {
        JsonObject exp = Expected("stale-fallback");
        string dir = TempDir();
        try
        {
            InstallCache(dir, StaleCache);
            var stub = new RecordingFetch(SourcePayload(), throws: true);
            (PricedRollup priced, _) = Run(dir, stub);
            Assert.Equal(1, stub.Calls);
            Assert.True((bool)exp["fetched"]!);
            AssertPriced(exp, priced);
            Assert.Equal((string?)exp["fetchedAt"], CurrentFetchedAt(dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    // -- the three hardening behaviors --------------------------------------

    [Fact]
    public void FetchedEmpty()
    {
        JsonObject exp = Expected("fetched-empty");
        string dir = TempDir();
        try
        {
            InstallCache(dir, StaleCache);
            string empty = LoadFixture("source-openrouter-empty.json").ToJsonString();
            var stub = new RecordingFetch(empty);
            (PricedRollup priced, _) = Run(dir, stub);
            // the fetch is attempted once, its unusable payload discarded: the
            // stale catalog is served and the cache file is NOT overwritten
            Assert.Equal(1, stub.Calls);
            Assert.True((bool)exp["fetched"]!);
            AssertPriced(exp, priced);
            Assert.Equal((string?)exp["fetchedAt"], CurrentFetchedAt(dir));
            Assert.Equal((string)Request["fetchedAtStale"]!, CurrentFetchedAt(dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void FetchedInvalidJson()
    {
        // a 2xx body that is not valid JSON follows the same stale-fallback rule
        JsonObject exp = Expected("fetched-empty");
        string dir = TempDir();
        try
        {
            InstallCache(dir, StaleCache);
            var stub = new RecordingFetch("<html>error</html>");
            (PricedRollup priced, _) = Run(dir, stub);
            Assert.Equal(1, stub.Calls);
            AssertPriced(exp, priced);
            Assert.Equal((string)Request["fetchedAtStale"]!, CurrentFetchedAt(dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void CachedEmpty()
    {
        JsonObject exp = Expected("cached-empty");
        JsonObject emptyCache = LoadFixture("cache-fresh-empty.json");
        string dir = TempDir();
        try
        {
            InstallCache(dir, emptyCache);
            var stub = new RecordingFetch(SourcePayload());
            (PricedRollup priced, _) = Run(dir, stub);
            // a fresh cache that probes to zero prices is unusable, so a fetch is
            // issued and the cache is rewritten with fetchedAt = now
            Assert.Equal(1, stub.Calls);
            Assert.True((bool)exp["fetched"]!);
            AssertPriced(exp, priced);
            Assert.Equal((string?)exp["fetchedAt"], CurrentFetchedAt(dir));
            Assert.Equal((string)Request["fetchedAtFetched"]!, CurrentFetchedAt(dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    [Fact]
    public void LiveReload()
    {
        JsonObject exp = LoadFixture("expected", "live-reload.json");
        Assert.Equal("live-reload", (string)exp["behavior"]!);
        string dir = TempDir();
        try
        {
            // a mutable clock the test advances between the two queries
            DateTimeOffset first = DateTimeOffset.Parse(
                (string)Request["now"]!, null, System.Globalization.DateTimeStyles.AdjustToUniversal);
            DateTimeOffset second = DateTimeOffset.Parse(
                (string)Request["nowSecond"]!, null, System.Globalization.DateTimeStyles.AdjustToUniversal);
            DateTimeOffset current = first;

            var stub = new RecordingFetch(SourcePayload());
            Func<string, PricingSource> make = SourceFactory(RequestSource());
            using Pricing pricing = SkopliClient.CreatePricing(new PricingOptions
            {
                Mode = RequestMode(),
                Sources = new[] { make(SourceUrl) },
                CacheDir = dir,
                Fetch = stub.Fetch,
                Clock = () => current,
                TtlMs = TtlMs(),
            });

            // both queries' priced state and USD are driven from the committed
            // expected fields (every field is checked)
            bool wantPriced = (bool)exp["priced"]!;
            double wantUsd = (double)exp["pricedUsd"]!;

            PricedRollup first1 = pricing.PriceRollups(new[] { RequestRollup() }).Single();
            PriceHit hit1 = Assert.IsType<PriceHit>(first1.Pricing);
            Assert.Equal(wantPriced, hit1.Priced);
            Assert.Equal(wantUsd, hit1.Usd, 10);
            Assert.Equal((string)exp["fetchedAtFirst"]!, CurrentFetchedAt(dir));

            // advance past the TTL window; the same instance must reload and the
            // now-stale disk cache forces a second fetch
            current = second;
            PricedRollup second2 = pricing.PriceRollups(new[] { RequestRollup() }).Single();
            PriceHit hit2 = Assert.IsType<PriceHit>(second2.Pricing);
            Assert.Equal(wantPriced, hit2.Priced);
            Assert.Equal(wantUsd, hit2.Usd, 10);
            Assert.Equal((int)exp["fetchesTotal"]!, stub.Calls);
            Assert.Equal((string)exp["fetchedAtSecond"]!, CurrentFetchedAt(dir));
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    // a Fetch that counts calls and serves the same payload each time
    private sealed class CountingFetch
    {
        private readonly string _payload;

        public CountingFetch(string payload) => _payload = payload;

        public int Calls { get; private set; }

        public Func<string, Task<string>> Fetch => _ =>
        {
            Calls++;
            return Task.FromResult(_payload);
        };
    }

    /// <summary>
    /// Proves an expired reload that FAILS is not memoized: load 1 succeeds, the
    /// clock advances past the TTL, the first reload throws (the cache directory
    /// is briefly replaced by a file so the post-fetch cache write fails and the
    /// whole load errors), and a later query retries the reload and succeeds.
    /// Expiry stays in force across the failure; the fetch counts prove the retry.
    /// </summary>
    [Fact]
    public void FailedReloadRetries()
    {
        string dir = TempDir();
        try
        {
            DateTimeOffset first = DateTimeOffset.Parse(
                (string)Request["now"]!, null, System.Globalization.DateTimeStyles.AdjustToUniversal);
            DateTimeOffset second = DateTimeOffset.Parse(
                (string)Request["nowSecond"]!, null, System.Globalization.DateTimeStyles.AdjustToUniversal);
            DateTimeOffset current = first;

            var stub = new CountingFetch(SourcePayload());
            Func<string, PricingSource> make = SourceFactory(RequestSource());
            using Pricing pricing = SkopliClient.CreatePricing(new PricingOptions
            {
                Mode = RequestMode(),
                Sources = new[] { make(SourceUrl) },
                CacheDir = dir,
                Fetch = stub.Fetch,
                Clock = () => current,
                TtlMs = TtlMs(),
            });

            // load 1 succeeds and stores the cache
            PricedRollup load1 = pricing.PriceRollups(new[] { RequestRollup() }).Single();
            Assert.True(Assert.IsType<PriceHit>(load1.Pricing).Priced);
            Assert.Equal(1, stub.Calls);

            // advance past the TTL so the next query reloads; make the cache write
            // fail by replacing the cache directory with a regular file, so the
            // reload's StoreCached throws and the whole load errors (not memoized)
            current = second;
            Directory.Delete(dir, true);
            File.WriteAllText(dir, "not a directory");

            Assert.ThrowsAny<Exception>(() => pricing.PriceRollups(new[] { RequestRollup() }));
            Assert.Equal(2, stub.Calls);

            // the failure is not memoized: restore the cache directory and a later
            // query retries the reload and succeeds
            File.Delete(dir);
            Directory.CreateDirectory(dir);

            PricedRollup retry = pricing.PriceRollups(new[] { RequestRollup() }).Single();
            Assert.True(Assert.IsType<PriceHit>(retry.Pricing).Priced);
            Assert.Equal(3, stub.Calls);
        }
        finally
        {
            if (File.Exists(dir))
            {
                File.Delete(dir);
            }
            else if (Directory.Exists(dir))
            {
                Directory.Delete(dir, true);
            }
        }
    }

    // -- table-driven per-source fetch ------------------------------------

    [Theory]
    [InlineData("source-openrouter.json", "openrouter", "pricing-openrouter.json")]
    [InlineData("source-litellm.json", "litellm", "pricing-litellm.json")]
    [InlineData("source-modelsdev.json", "models-dev", "pricing-models-dev.json")]
    public void PerSourceColdFetch(string fixture, string sourceName, string cacheName)
    {
        string dir = TempDir();
        try
        {
            string payload = LoadFixture(fixture).ToJsonString();
            var stub = new RecordingFetch(payload);
            Func<string, PricingSource> make = SourceFactory(sourceName);
            using Pricing pricing = SkopliClient.CreatePricing(new PricingOptions
            {
                Mode = RequestMode(),
                Sources = new[] { make($"https://{sourceName}.test/api") },
                CacheDir = dir,
                Fetch = stub.Fetch,
                Clock = FixedClock(),
                TtlMs = TtlMs(),
            });

            Assert.Equal(1, stub.Calls);
            // exact source provenance
            Assert.Contains(pricing.Catalogs(), c => c.Source == sourceName);
            // exact cache filename written for this source
            Assert.True(File.Exists(Path.Combine(dir, cacheName)));
            // shared rollup prices to 2.25 through this source's core parser
            PricedRollup priced = pricing.PriceRollups(new[] { RequestRollup() }).Single();
            PriceHit hit = Assert.IsType<PriceHit>(priced.Pricing);
            Assert.Equal(2.25, hit.Usd, 10);
        }
        finally
        {
            Directory.Delete(dir, true);
        }
    }

    // -- concurrent close/dispose race protection----------------------------

    [Fact]
    public void ConcurrentDisposeIsExactlyOnce()
    {
        using Pricing pricing = SkopliClient.CreatePricing(new PricingOptions
        {
            Mode = PricingMode.Calculate,
            Sources = Array.Empty<PricingSource>(),
            Catalogs = new[]
            {
                new Catalog { Source = "litellm", FetchedAt = "2026-08-01T00:00:00.000Z", Format = "litellm", Payload = ReadCatalog("litellm") },
            },
        });

        // many concurrent Dispose calls must free exactly once (the lock + the
        // _disposed guard); a repeated Dispose is a no-op
        Parallel.For(0, 32, _ => pricing.Dispose());
        pricing.Dispose();
    }

    [Fact]
    public async Task DisposeDuringQueuedAsyncCallDoesNotRaceFree()
    {
        Pricing pricing = SkopliClient.CreatePricing(new PricingOptions
        {
            Mode = PricingMode.Calculate,
            Sources = Array.Empty<PricingSource>(),
            Catalogs = new[]
            {
                new Catalog { Source = "litellm", FetchedAt = "2026-08-01T00:00:00.000Z", Format = "litellm", Payload = ReadCatalog("litellm") },
            },
        });

        // start async calls, then dispose racing the queued Task.Run bodies. The
        // async wrappers acquire the same lock as Dispose, so a queued native
        // call cannot overlap the free: each call either completes or throws
        // ObjectDisposedException, never a use-after-free.
        var tasks = new List<Task>();
        for (int i = 0; i < 16; i++)
        {
            tasks.Add(Task.Run(async () =>
            {
                try
                {
                    await pricing.LookupModelAsync("gpt-5");
                }
                catch (ObjectDisposedException)
                {
                    // expected once disposal wins the race
                }
            }));
        }
        pricing.Dispose();
        await Task.WhenAll(tasks);
        pricing.Dispose();
    }

    // -- PriceRollups full gold equality ------------------------------------

    private static string ReadCatalog(string name) =>
        File.ReadAllText(GoldenPath("pricing", "catalogs", $"{name}.json"));

    [Fact]
    public void PriceRollupsMatchesGoldFully()
    {
        const string pinned = "2026-08-01T00:00:00.000Z";
        var options = new PricingOptions
        {
            Mode = PricingMode.Calculate,
            Sources = Array.Empty<PricingSource>(),
            Catalogs = new[]
            {
                new Catalog { Source = "openrouter", FetchedAt = pinned, Format = "openrouter", Payload = ReadCatalog("openrouter") },
                new Catalog { Source = "litellm", FetchedAt = pinned, Format = "litellm", Payload = ReadCatalog("litellm") },
            },
        };
        using Pricing pricing = SkopliClient.CreatePricing(options);

        // the untouched expected gold rollups array is the whole target
        JsonNode gold = JsonNode.Parse(File.ReadAllText(GoldenPath("pricing", "basic", "expected-priced-rollup.json")))!;
        JsonArray goldRollups = gold["rollups"]!.AsArray();

        // input: a raw copy of each gold rollup with ONLY the pricing field removed
        var inputs = new List<Rollup>();
        foreach (JsonNode? node in goldRollups)
        {
            var bucket = (JsonObject)node!.DeepClone();
            bucket.Remove("pricing");
            inputs.Add(bucket.Deserialize<Rollup>(new JsonSerializerOptions())!);
        }

        IReadOnlyList<PricedRollup> priced = pricing.PriceRollups(inputs);

        // encode the actual returned values back to the wire tree and compare the
        // COMPLETE tree against the untouched expected rollups:
        // no field removal on the output, no selective assertions
        var actual = new JsonArray();
        foreach (PricedRollup pr in priced)
        {
            actual.Add(WireShape(pr));
        }
        Assert.Equal(Canonical(goldRollups), Canonical(actual));
    }

    // WireShape encodes a returned PricedRollup back into the on-the-wire JSON
    // shape (flat rollup fields plus a pricing object), so the full returned tree
    // can be compared against the gold rollups array.
    private static JsonObject WireShape(PricedRollup pr)
    {
        JsonObject obj = JsonSerializer.SerializeToNode(pr.Rollup, PriceOptions)!.AsObject();
        obj["pricing"] = pr.Pricing switch
        {
            PriceHit hit => HitNode(hit),
            PriceMiss miss => MissNode(miss),
            _ => throw new InvalidOperationException("unknown pricing kind"),
        };
        return obj;
    }

    private static JsonObject HitNode(PriceHit hit)
    {
        var obj = new JsonObject { ["priced"] = true, ["model"] = hit.Model };
        if (hit.Key is not null)
        {
            obj["key"] = hit.Key;
        }
        if (hit.Price is not null)
        {
            obj["price"] = JsonSerializer.SerializeToNode(hit.Price, PriceOptions);
        }
        if (hit.Source is not null)
        {
            obj["source"] = hit.Source;
        }
        if (hit.FetchedAt is not null)
        {
            obj["fetchedAt"] = hit.FetchedAt;
        }
        obj["usd"] = hit.Usd;
        if (hit.TieredAggregate)
        {
            obj["tieredAggregate"] = true;
        }
        if (hit.Models is not null)
        {
            var arr = new JsonArray();
            foreach (string m in hit.Models)
            {
                arr.Add(m);
            }
            obj["models"] = arr;
        }
        return obj;
    }

    private static JsonObject MissNode(PriceMiss miss)
    {
        var obj = new JsonObject { ["priced"] = false, ["model"] = miss.Model };
        var attempted = new JsonArray();
        foreach (string a in miss.Attempted)
        {
            attempted.Add(a);
        }
        obj["attempted"] = attempted;
        if (miss.Reason is not null)
        {
            obj["reason"] = miss.Reason;
        }
        if (miss.Key is not null)
        {
            obj["key"] = miss.Key;
        }
        if (miss.Usd is { } usd)
        {
            obj["usd"] = usd;
        }
        return obj;
    }

    // -- shared golden constants contract -----------------------------------

    /// <summary>
    /// The shared golden constants contract (constants.json): the facade's own
    /// source names, formats, URLs, priority order, TTL default, fetch timeout,
    /// and cache filenames must equal the golden values so no facade's literals
    /// drift.
    /// </summary>
    [Fact]
    public void LifecycleConstantsMatchGolden()
    {
        JsonObject constants = LoadFixture("constants.json");

        var urlByName = new Dictionary<string, string>
        {
            ["openrouter"] = PricingSource.OpenRouter().Url,
            ["litellm"] = PricingSource.LiteLlm().Url,
            ["models-dev"] = PricingSource.ModelsDev().Url,
        };
        var factoryByName = new Dictionary<string, PricingSource>
        {
            ["openrouter"] = PricingSource.OpenRouter(),
            ["litellm"] = PricingSource.LiteLlm(),
            ["models-dev"] = PricingSource.ModelsDev(),
        };

        JsonArray sources = constants["sources"]!.AsArray();
        JsonArray priority = constants["priorityOrder"]!.AsArray();

        // the sources array is in the canonical priority order
        for (int i = 0; i < sources.Count; i++)
        {
            Assert.Equal((string)priority[i]!, (string)sources[i]!["name"]!);
        }

        string pattern = (string)constants["cacheFileNamePattern"]!;
        foreach (JsonNode? node in sources)
        {
            JsonObject spec = node!.AsObject();
            string name = (string)spec["name"]!;
            PricingSource source = factoryByName[name];
            Assert.Equal(name, source.Name);
            Assert.Equal((string)spec["format"]!, source.Format);
            Assert.Equal((string)spec["url"]!, source.Url);
            Assert.Equal((string)spec["url"]!, urlByName[name]);
            Assert.Equal(pattern.Replace("{name}", name), (string)spec["cacheFileName"]!);
            // compare the production filename formula (SourceLoader.CacheFileName),
            // not just the fixture pattern, so the real formula cannot drift unseen
            Assert.Equal((string)spec["cacheFileName"]!, Skopli.Internal.SourceLoader.CacheFileName(name));
        }

        Assert.Equal(Skopli.Internal.SourceLoader.DefaultTtlMs, (long)constants["defaultTtlMs"]!);
        Assert.Equal(Skopli.Internal.SourceLoader.FetchTimeoutMs, (int)constants["fetchTimeoutMs"]!);
    }

    // -- LookupModel hit + miss ---------------------------------------------

    [Fact]
    public void LookupModelHitAndMiss()
    {
        const string pinned = "2026-08-01T00:00:00.000Z";
        using Pricing pricing = SkopliClient.CreatePricing(new PricingOptions
        {
            Mode = PricingMode.Calculate,
            Sources = Array.Empty<PricingSource>(),
            Catalogs = new[]
            {
                new Catalog { Source = "litellm", FetchedAt = pinned, Format = "litellm", Payload = ReadCatalog("litellm") },
            },
        });

        PriceLookup hit = pricing.LookupModel("gpt-5");
        PriceHit gpt5 = Assert.IsType<PriceHit>(hit);
        Assert.True(gpt5.Priced);
        Assert.Equal("litellm", gpt5.Source);

        PriceLookup miss = pricing.LookupModel("totally-unknown-model-9000");
        PriceMiss unknown = Assert.IsType<PriceMiss>(miss);
        Assert.False(unknown.Priced);
    }

    // -- helpers ------------------------------------------------------------

    private static readonly JsonSerializerOptions PriceOptions = new()
    {
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull,
    };

    private static string Canonical(JsonNode node)
    {
        switch (node)
        {
            case JsonObject obj:
                var sorted = new JsonObject();
                foreach (var kv in obj.OrderBy(p => p.Key, StringComparer.Ordinal))
                {
                    sorted[kv.Key] = kv.Value is null ? null : JsonNode.Parse(Canonical(kv.Value));
                }
                return sorted.ToJsonString();
            case JsonArray arr:
                var outArr = new JsonArray();
                foreach (JsonNode? item in arr)
                {
                    outArr.Add(item is null ? null : JsonNode.Parse(Canonical(item)));
                }
                return outArr.ToJsonString();
            default:
                JsonValue value = node.AsValue();
                if (value.TryGetValue(out double d))
                {
                    return JsonValue.Create(d).ToJsonString();
                }
                return value.ToJsonString();
        }
    }
}

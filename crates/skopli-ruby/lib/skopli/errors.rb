# frozen_string_literal: true

module Skopli
  # Base of the gem's error hierarchy.
  class Error < StandardError; end

  # A programmer mistake: bad date/tz/dimension, malformed argument JSON. Also a
  # kind of ArgumentError, matching Ruby's idiom for bad arguments.
  class InvalidArgumentError < ArgumentError; end

  # A pricing/catalog failure (bad catalog JSON, unknown source format, ...).
  class CatalogError < Error; end

  # Internal glue: the native extension raises a single `Skopli::NativeError`
  # whose message is `"<kind>:<message>"` (kind one of invalid_argument / catalog
  # / internal). `raise_typed` strips the tag and re-raises the idiomatic
  # subclass, so the Rust side stays idiom-free (mirrors the PyO3 binding's shape).
  module Errors
    module_function

    # Run a native call, translating `NativeError` into the typed hierarchy.
    def translate
      yield
    rescue Skopli::NativeError => e
      kind, _, message = e.message.partition(":")
      raise_typed(kind, message)
    end

    def raise_typed(kind, message)
      case kind
      when "invalid_argument"
        raise InvalidArgumentError, message
      when "catalog"
        raise CatalogError, message
      else
        raise Error, message
      end
    end
  end
end

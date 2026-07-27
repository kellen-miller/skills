# Briefing Schema

Use this schema to create the temporary JSON consumed by
`scripts/render_briefing.py`. The renderer supplies `repository_root`,
`work_item`, `branch`, `head`, `tree_state`, `fingerprint`, and `generated_at`
from the live repository.

## Source reference

Use source references wherever the page makes an implementation claim:

```json
{
  "path": "internal/orders/service.go",
  "label": "Order service",
  "symbol": "Service.Place",
  "line": 84
}
```

Only `path` is required. Keep paths repository-relative. Use `line` only when
it is verified against the rendered snapshot.

## Complete shape

```json
{
  "schema_version": 1,
  "title": "Human-readable feature name",
  "summary": "One sentence describing the implemented capability.",
  "snapshot": {
    "repository": "owner/repository",
    "base_ref": "origin/main"
  },
  "orientation": {
    "why": "The user or system problem this solves.",
    "before": "The relevant behavior before this implementation.",
    "after": "The behavior and ownership after this implementation.",
    "entry_point": {
      "path": "internal/orders/handler.go",
      "symbol": "Handler.PlaceOrder",
      "line": 40
    },
    "concepts": [
      {
        "name": "Reservation",
        "explanation": "A temporary inventory claim owned by the order flow."
      }
    ]
  },
  "components": [
    {
      "id": "handler",
      "name": "Order handler",
      "role": "Validates the request boundary and starts the order flow.",
      "owns": [
        "Transport validation",
        "Mapping the response"
      ],
      "invariants": [
        "Invalid requests never enter the service"
      ],
      "connects_to": [
        "service"
      ],
      "sources": [
        {
          "path": "internal/orders/handler.go",
          "symbol": "Handler.PlaceOrder"
        }
      ]
    },
    {
      "id": "service",
      "name": "Order service",
      "role": "Coordinates reservation and persistence in business order.",
      "owns": [
        "The order lifecycle"
      ],
      "invariants": [
        "Persistence happens only after inventory is reserved"
      ],
      "connects_to": [],
      "sources": [
        {
          "path": "internal/orders/service.go",
          "symbol": "Service.Place"
        }
      ]
    }
  ],
  "flows": [
    {
      "id": "happy-path",
      "name": "Successful order",
      "summary": "A valid request reserves inventory and persists an order.",
      "steps": [
        {
          "component": "handler",
          "title": "Validate the request",
          "detail": "The transport boundary rejects malformed identifiers.",
          "source": {
            "path": "internal/orders/handler.go",
            "symbol": "Handler.PlaceOrder"
          }
        },
        {
          "component": "service",
          "title": "Coordinate the order",
          "detail": "The service reserves inventory before persisting.",
          "source": {
            "path": "internal/orders/service.go",
            "symbol": "Service.Place"
          }
        }
      ]
    }
  ],
  "decisions": [
    {
      "title": "Keep ordering in the service",
      "choice": "One visible executor",
      "reason": "The side-effect order is a business invariant.",
      "tradeoffs": [
        "The method is longer, but the lifecycle remains readable."
      ],
      "sources": [
        {
          "path": ".agent/work/orders/decision.md"
        },
        {
          "path": "internal/orders/service.go",
          "symbol": "Service.Place"
        }
      ]
    }
  ],
  "change_recipes": [
    {
      "goal": "Change request validation",
      "start_here": {
        "path": "internal/orders/handler.go",
        "symbol": "Handler.PlaceOrder"
      },
      "steps": [
        "Change validation at the transport boundary.",
        "Update the request-focused handler tests."
      ],
      "watch_for": [
        "Do not duplicate transport validation in the service."
      ],
      "tests": [
        "go test ./internal/orders/..."
      ]
    }
  ],
  "verification": [
    {
      "behavior": "Inventory is reserved before persistence",
      "tests": [
        {
          "path": "internal/orders/service_test.go",
          "symbol": "TestServicePlaceReservesBeforePersisting"
        }
      ],
      "commands": [
        "go test ./internal/orders/..."
      ],
      "evidence": [
        "The final validation completed without failures."
      ]
    }
  ],
  "questions": [
    {
      "prompt": "Where should malformed identifiers be rejected?",
      "choices": [
        "The handler boundary",
        "The persistence adapter"
      ],
      "answer": 0,
      "explanation": "The handler owns untrusted transport input.",
      "source": {
        "path": "internal/orders/handler.go",
        "symbol": "Handler.PlaceOrder"
      }
    }
  ],
  "risks": [
    {
      "title": "Reservation expiration",
      "detail": "The current flow relies on the inventory service timeout.",
      "mitigation": "Watch reservation timeout metrics during rollout."
    }
  ],
  "glossary": [
    {
      "term": "Reservation",
      "definition": "A temporary inventory claim made before order persistence."
    }
  ]
}
```

## Content rules

- Use stable lowercase IDs for components and flows.
- Point every `connects_to` value and flow-step `component` at a declared
  component ID.
- Keep component roles distinct. If two components appear to own the same
  behavior, inspect the implementation instead of smoothing over the overlap.
- Write flow steps in runtime order. Include side effects where they occur.
- Use exact commands that were actually run.
- Keep choices plausible and similar in length. Do not reveal the answer through
  formatting.
- Use empty arrays for inapplicable `risks` and `glossary`; all other top-level
  collections must contain at least one item.
- Provide at least three questions in real briefings even though the renderer
  accepts one for small fixtures and schema tests.

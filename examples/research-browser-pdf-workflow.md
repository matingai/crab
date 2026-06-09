# Example: Research Browser PDF Workflow

This workflow is meant to show Crab as a research assistant runtime with browser and PDF
tooling in the same local agent loop.

## What This Shows

- Browser navigation and page extraction.
- PDF inspection and summarization.
- Evidence gathering before synthesis.
- Goal-state memory for open questions, risks, and verified facts.
- Worker delegation for bounded reading or comparison tasks.

## Setup

Use public sources or demo documents. Keep private PDFs out of public recordings.

```bash
export OPENAI_API_KEY="your-api-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"
export HERMES_RS_MODEL="gpt-4.1-mini"
```

## Demo Prompt

```bash
cargo run -- chat --prompt "Research the public docs and local README for this project. Explain what makes Crab different from a simple chat wrapper, cite the local files you used, and list the strongest open-source positioning angles."
```

If browser tools are configured for your environment, a broader prompt can be:

```bash
cargo run -- chat --prompt "Compare Crab's positioning with two public local-agent/runtime projects. Gather evidence first, then summarize differentiators and risks."
```

## What To Narrate

Crab is strongest when the demo shows the process, not only the answer:

- the timeline shows what was inspected;
- tool observations are summarized before being folded into goal state;
- the main agent can ask workers to read or verify bounded material;
- the final response should distinguish evidence from positioning judgment.

## Suggested Output Shape

Ask Crab for:

```text
Return a concise launch memo with: thesis, evidence, comparable projects, strongest claims,
risky claims to avoid, and recommended next demo.
```

## Safety Notes

- Do not paste private browser cookies, private PDFs, or proprietary papers into public
  demos.
- Prefer public links and generated demo PDFs when recording.
- Treat model-generated comparison claims as draft positioning until manually checked.

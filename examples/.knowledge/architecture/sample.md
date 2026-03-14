---
keywords: [authentication, login, OAuth, session]
category: architecture
---

# Login Authentication Flow

## Entry: OAuth Provider Configuration
keywords: [OAuth, provider, config, Google, Apple]

OAuth providers are configured in `config/auth_providers.yaml`.
Supported providers: Google and Apple Sign-In.
The `AuthProviderRegistry` class in `src/auth/registry.ts` manages provider lifecycle.

## Entry: Session Management
keywords: [session, token, refresh, JWT]

Sessions use JWT with 15-minute access tokens and 7-day refresh tokens.
The `SessionManager` in `src/auth/session.ts` handles token rotation.
Refresh tokens are stored in HttpOnly cookies.

## Entry: Login Error Handling
keywords: [login, error, retry, lockout]

After 5 failed attempts, the account is locked for 30 minutes.
Error codes are defined in `src/auth/errors.ts`.
The `LoginGuard` middleware handles rate limiting.

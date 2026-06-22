# Jahan Nama / Webotel API Documentation

This document describes the HTTP API used to authenticate a Jahan Nama/Webotel user and retrieve the user's remaining internet traffic. It is written for anyone building an independent client.

## Base URL

```text
https://qomservice.webotel.ir
```

## Endpoints

| Purpose | Method | Path |
| --- | --- | --- |
| Authenticate user | `POST` | `/api/login/AuthenticateWeb` |
| Get remaining traffic | `GET` | `/api/BaseInfo/GetUserRemain` |

## Common Headers

Clients should send:

```http
Accept: application/json
User-Agent: Mozilla/5.0
```

The API returns JSON responses.

## Authentication

Authenticate with the user's username and password to receive an access token.

### Request

```http
POST /api/login/AuthenticateWeb HTTP/1.1
Host: qomservice.webotel.ir
Accept: application/json
User-Agent: Mozilla/5.0
Content-Type: application/x-www-form-urlencoded
```

Form fields:

| Field | Required | Description |
| --- | --- | --- |
| `Username` | Yes | The user's account username. |
| `Password` | Yes | The user's account password. |
| `DeviceTypeEnum` | Yes | Device type value. Existing clients use `4`. |
| `IP` | No | IP value. Existing clients send an empty string. |

Example:

```sh
curl -X POST 'https://qomservice.webotel.ir/api/login/AuthenticateWeb' \
  -H 'Accept: application/json' \
  -H 'User-Agent: Mozilla/5.0' \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  --data-urlencode 'Username=YOUR_USERNAME' \
  --data-urlencode 'Password=YOUR_PASSWORD' \
  --data-urlencode 'DeviceTypeEnum=4' \
  --data-urlencode 'IP='
```

### Response

The response is expected to be a JSON object containing a token:

```json
{
  "Token": "ACCESS_TOKEN"
}
```

The response may include additional fields. A client should treat authentication as successful only when `Token` exists and is a non-empty string.

## Get Remaining Traffic

Use the authentication token to fetch the remaining traffic.

### Request

```http
GET /api/BaseInfo/GetUserRemain?Token=ACCESS_TOKEN HTTP/1.1
Host: qomservice.webotel.ir
Accept: application/json
User-Agent: Mozilla/5.0
```

Query parameters:

| Parameter | Required | Description |
| --- | --- | --- |
| `Token` | Yes | Token returned by `/api/login/AuthenticateWeb`. |

Example:

```sh
curl 'https://qomservice.webotel.ir/api/BaseInfo/GetUserRemain?Token=ACCESS_TOKEN' \
  -H 'Accept: application/json' \
  -H 'User-Agent: Mozilla/5.0'
```

### Response

The response is expected to be a JSON object containing `RemainTraffic`:

```json
{
  "RemainTraffic": 1536
}
```

The response may include additional fields. `RemainTraffic` is the remaining traffic amount in megabytes.

Known accepted value shapes:

```json
{ "RemainTraffic": 1536 }
```

```json
{ "RemainTraffic": "1536" }
```

Client implementations should accept `RemainTraffic` as either a JSON number or a numeric string. Other JSON types should be treated as invalid for this field.

## Recommended Client Flow

1. Send username and password to `/api/login/AuthenticateWeb`.
2. Read the `Token` field from the authentication response.
3. Store the token securely for reuse.
4. Call `/api/BaseInfo/GetUserRemain` with `Token` as a query parameter.
5. Read `RemainTraffic` from the response.
6. If the token request fails with `401 Unauthorized` or `403 Forbidden`, authenticate again and retry the remaining-traffic request once.
7. If the remaining-traffic response does not contain a valid `RemainTraffic` value, treat the response as an API error.

## Token Handling

The API token is passed as a query parameter, not as an `Authorization` header.

```text
/api/BaseInfo/GetUserRemain?Token=ACCESS_TOKEN
```

No refresh-token endpoint is known from the observed client behavior. When a token expires or is rejected, clients should authenticate again with username and password.

Because the token grants access to account information, clients should store it securely and avoid logging it.

## Traffic Unit Formatting

`RemainTraffic` is returned in megabytes.

Example conversions:

| Raw value | Meaning | Display example |
| --- | --- | --- |
| `512` | 512 MB | `512.00 MB` |
| `1536` | 1536 MB | `1.50 GB` |

A common display rule is:

- If `RemainTraffic` is less than `1024`, show it as MB.
- If `RemainTraffic` is `1024` or greater, divide by `1024` and show it as GB.

## Error Handling

Clients should handle these cases:

| Case | Recommended handling |
| --- | --- |
| Network error or timeout | Report a request failure and allow retry. |
| Non-2xx HTTP status | Treat as request failure. Re-authenticate once for `401` or `403`. |
| Authentication response missing `Token` | Treat as authentication failure. |
| Remaining-traffic response missing `RemainTraffic` | Treat as unexpected API response. |
| `RemainTraffic` is not numeric | Treat as unexpected API response. |
| Invalid JSON response | Treat as unexpected API response. |


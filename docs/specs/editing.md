The users are allowed to edit their availability info (name and time slots).

- Each availability answer (response row) has a randomly-generated _edit token_.
- The edit token is stored in the user's browser via a path-specific (`/m/{id}`) cookie.
  The cookie expires after 90 days.
- The user is allowed to only edit availability info for which they have the token.

If the user loses the token, so be it. They can submit a new answer if they want.

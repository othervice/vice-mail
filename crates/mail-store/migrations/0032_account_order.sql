-- Stage 22 (decision 0078): the user's order of the accounts.
--
-- `account.position` (class Einstellung, synchronised as the field `position` of the account
--   record): where the account stands in every list of accounts, lowest first. NULL until the
--   user moves an account: such accounts follow the placed ones in the order they were created,
--   which is the order of every list before this migration, so nothing moves by itself. "Move"
--   gives every account a fresh position above all earlier ones (`accounts::reorder`), so the
--   newest order wins as a whole across devices (see decision 0078). The list reads all rows
--   (a handful), so no index.
--
-- `account.display_name` keeps its column and becomes the name the account has in the lists
--   (decision 0078); the sender name lives in `identity.name` since stage 2.

ALTER TABLE account ADD COLUMN position INTEGER;

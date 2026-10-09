-- Foo fixture
-- (upstream added a header line)
local M = {}

function M.one(a)   	
	return a + 1
end

-- a comment between functions

function M.two(b)
	return b * 20
end

function M.three(c)
	return c - 3
end

return M

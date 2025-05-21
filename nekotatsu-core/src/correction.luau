local GUYA_PARSERS = {
	DANKE = true,
	GUYACUBARI = true,
	HACHIRUMI = true,
	MAHOUSHOUJOBU = true,
}

local UNIQUE_RELATIVE: {
	[string]: (...string) -> string,
} = {
	MANGADEX = function(_domain, url)
		return select(1, url:gsub("/manga/", "/title/"))
	end,
	WEEBCENTRAL = function(domain, url)
		url = url:gsub("/series/", "")
		return (url:split("/"))[1]
	end,
}

function correct_relative_url(name: string, domain: string, url: string)
	local unique = UNIQUE_RELATIVE[name]
	if unique then
		return unique(domain, url)
	end

	if GUYA_PARSERS[name] then
		return `https://{domain}/api/series/{url}`
	end

	return `{domain}{url}`
end

local UNIQUE_PUBLIC: {
	[string]: (...string) -> string,
} = {
	WEEBCENTRAL = function(domain, url)
		return `https://{domain}/series{url}`
	end,
}

function correct_public_url(name: string, domain: string, url: string)
	local unique = UNIQUE_PUBLIC[name]
	if unique then
		return unique(domain, url)
	end

	if GUYA_PARSERS[name] then
		return `https://{domain}/read/manga/{url}`
	end

	return `https://{domain}{url:gsub("^/+", "", 1)}`
end

function correct_manga_identifier(name: string, current: string)
	if name == "MANGADEX" then
		return select(1, current:gsub("/title/", ""))
	end

	return current
end

function correct_chapter_identifier(name: string, current: string)
	if name == "MANGADEX" then
		return select(1, current:gsub("/chapter/", ""))
	end

	return current
end

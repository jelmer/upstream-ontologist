from typing import Iterator

from upstream_ontologist import UpstreamDatum, UpstreamMetadata, UpstreamPackage

def drop_vcs_in_scheme(url: str) -> str: ...
def unsplit_vcs_url(repo_url: str, branch: str | None, subpath: str | None) -> str: ...
def probe_gitlab_host(hostname: str) -> bool: ...
def is_gitlab_site(hostname: str, net_access: bool | None = None) -> bool: ...
def guess_repo_from_url(url: str, net_access: bool | None = None) -> str | None: ...
def probe_gitlabb_host(hostname: str) -> bool: ...
def find_public_repo_url(url: str, net_access: bool | None = None) -> str | None: ...
def browse_url_from_repo_url(
    url: str,
    branch: str | None = None,
    subpath: str | None = None,
    net_access: bool | None = None,
) -> str | None: ...
def plausible_vcs_url(url: str) -> bool: ...
def plausible_vcs_browse_url(url: str) -> bool: ...
def probe_upstream_branch_url(url: str, version: str | None = None) -> bool | None: ...
def canonical_git_repo_url(url: str, net_access: bool | None = None) -> str: ...
def check_repository_url_canonical(url: str, version: str | None = None) -> str: ...
def guess_from_launchpad(
    package: str, distribution: str | None = None, suite: str | None = None
) -> Iterator[tuple[str, UpstreamDatum]]: ...
def guess_from_aur(package: str) -> Iterator[tuple[str, UpstreamDatum]]: ...
def guess_from_gobo(package: str) -> Iterator[tuple[str, str]]: ...
def guess_from_hackage(package: str) -> Iterator[UpstreamMetadata]: ...
def guess_from_repology(project: str) -> Iterator[tuple[str, str]]: ...
def get_repology_metadata(name: str, distro: str | None = None): ...
def extract_pecl_package_name(url: str) -> str | None: ...
def extract_sf_project_name(url: str) -> str | None: ...
def known_bad_guess(datum: UpstreamDatum) -> bool: ...
def url_from_svn_co_command(command: bytes) -> str | None: ...
def url_from_git_clone_command(command: bytes) -> str | None: ...
def url_from_fossil_clone_command(command: bytes) -> str | None: ...
def url_from_cvs_co_command(command: bytes) -> str | None: ...
def url_from_vcs_command(command: bytes) -> str | None: ...
def find_forge(url: str, net_access: bool | None = None) -> Forge | None: ...
def repo_url_from_merge_request_url(
    url: str, net_access: bool | None = None
) -> str | None: ...
def bug_database_from_issue_url(
    url: str, net_access: bool | None = None
) -> str | None: ...
def guess_bug_database_url_from_repo_url(
    url: str, net_access: bool | None = None
) -> str | None: ...
def bug_database_url_from_bug_submit_url(
    url: str, net_access: bool | None = None
) -> str | None: ...
def bug_submit_url_from_bug_database_url(
    url: str, net_access: bool | None = None
) -> str | None: ...
def check_bug_database_canonical(url: str, net_access: bool | None = None) -> str: ...
def check_bug_submit_url_canonical(url: str, net_access: bool | None = None) -> str: ...
def check_url_canonical(url: str) -> str: ...
def get_sf_metadata(project: str): ...
def debian_is_native(path: str) -> bool | None: ...
def metadata_from_itp_bug_body(body: str) -> list[UpstreamDatum]: ...
def load_json_url(http_url: str, timeout: int | None = None): ...
def fixup_rcp_style_git_repo_url(url: str) -> str: ...
def valid_debian_package_name(name: str) -> bool: ...
def debian_to_upstream_version(version: str) -> str: ...
def upstream_name_to_debian_source_name(upstream_name: str) -> str: ...
def upstream_version_to_debian_upstream_version(
    version: str, family: str | None = None
) -> str: ...

def upstream_package_to_debian_source_name(package: UpstreamPackage) -> str: ...
def upstream_package_to_debian_binary_name(package: UpstreamPackage) -> str: ...

class ParseError(Exception): ...
class NoSuchForgeProject(Exception): ...
class NoSuchRepologyProject(Exception): ...

class Forge:
    @classmethod
    def extend_metadata(
        cls, upstream_metadata: UpstreamMetadata, project: str, certainty: str
    ) -> None: ...

    repository_browse_can_be_homepage: bool

class GitHub(Forge): ...
class GitLab(Forge): ...
class SourceForge(Forge): ...
class Launchpad(Forge): ...

SECURE_SCHEMES: list[str]
KNOWN_GITLAB_SITES: list[str]

def find_secure_repo_url(url: str, branch: str | None = None, net_access: bool | None = None) -> str | None: ...
def sanitize_url(url: str) -> str: ...
def convert_cvs_list_to_str(cvs_list: list[str]) -> str: ...
def fixup_broken_git_details(url: str, branch: str | None = None, subpath: str | None = None) -> tuple[str, str | None, str | None]: ...
def guess_upstream_info(
    path: str, trust_package: bool = False
) -> list[UpstreamDatum]: ...
def guess_from_pecl_package(package: str) -> Iterator[tuple[str, UpstreamDatum]]: ...

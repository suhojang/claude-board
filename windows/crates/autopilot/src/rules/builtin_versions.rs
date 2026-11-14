//! 앱에 든 기술 스택 버전 목록 글. 원본: `Sources/AutopilotRules/BuiltinVersions.swift`(전체).
//! 공식 목록을 받지 못하고 하루 지난 캐시도 없을 때 "오래된 목록(날짜)"으로 쓰는 목록이다(`rules::versions::load_catalog` 의 `builtin` 인수).
//! 순수 상수만 있다(파일·시각·환경 없음). Swift 의 SwiftPM 리소스를 쓰지 않고 글자로 둔 것도 그대로다.
//!
//! 이식된 선언: `BUILTIN_CATALOG_JSON`(Swift `apBuiltinCatalogJSON`) · `builtin_catalog_data`(Swift `apBuiltinCatalogData`, UTF-8 바이트).

/// Swift `apBuiltinCatalogJSON` 글자 그대로. 그날 받은 공식 목록으로 `build_catalog` 가 만든 꼴이다.
pub const BUILTIN_CATALOG_JSON: &str = r##"{"at":1793286911,"items":{"fastapi":[{"lts":false,"rec":true,"v":"0.162"}],"java":[{"lts":true,"rec":true,"until":"2031-09","v":"25"},{"lts":true,"rec":false,"until":"2029-12","v":"21"},{"lts":true,"rec":false,"until":"2027-10","v":"17"}],"kotlin":[{"lts":false,"rec":true,"v":"2.4"}],"mysql":[{"lts":true,"rec":true,"until":"2034-04","v":"9.7"},{"lts":true,"rec":false,"until":"2032-04","v":"8.4"}],"nest":[{"lts":false,"rec":true,"v":"12.3"},{"lts":false,"rec":false,"v":"11.2"}],"next":[{"lts":false,"rec":true,"v":"16.4"}],"node":[{"lts":true,"rec":true,"until":"2029-04","v":"26"},{"lts":true,"rec":false,"until":"2028-04","v":"24"},{"lts":true,"rec":false,"until":"2027-04","v":"22"}],"postgresql":[{"lts":false,"rec":true,"until":"2030-11","v":"18"},{"lts":false,"rec":false,"until":"2029-11","v":"17"},{"lts":false,"rec":false,"until":"2028-11","v":"16"}],"python":[{"lts":false,"rec":true,"until":"2031-10","v":"3.15"},{"lts":false,"rec":false,"until":"2030-10","v":"3.14"},{"lts":false,"rec":false,"until":"2029-10","v":"3.13"}],"react":[{"lts":false,"rec":true,"v":"19.3"}],"spring-boot":[{"lts":false,"rec":true,"until":"2027-07","v":"4.1.2"},{"lts":false,"rec":false,"until":"2026-12","v":"4.0.9"}],"vite":[{"lts":false,"rec":true,"v":"8.3"}],"vue":[{"lts":false,"rec":true,"v":"3.5"}]},"notes":{"java":"목록에 없는 판: Java 27(LTS 가 아니거나 Spring Boot 4.1 의 범위 17 - 26 밖)"}}"##;

/// Swift `apBuiltinCatalogData`(`Data(apBuiltinCatalogJSON.utf8)`). `rules::versions::load_catalog` 의 `builtin` 인수로 넘긴다.
pub fn builtin_catalog_data() -> &'static [u8] {
    BUILTIN_CATALOG_JSON.as_bytes()
}

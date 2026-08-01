// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
//    You may obtain a copy of the License at
//
//        http://www.apache.org/licenses/LICENSE-2.0
//
//    Unless required by applicable law or agreed to in writing, software
//    distributed under the License is distributed on an "AS IS" BASIS,
//    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//    See the License for the specific language governing permissions and
//    limitations under the License.
// =============================================================================
//! Shared local-provider identity and URI constants.
// qubit-style: allow source-test-pair

/// Stable provider identity used by the host local filesystem.
pub(crate) const LOCAL_PROVIDER_ID: &str = "local-file";

/// URI scheme accepted by the local filesystem provider.
pub(crate) const FILE_SCHEME: &str = "file";

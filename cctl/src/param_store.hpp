#pragma once

#include "domain/parameters.hpp"

// 実行時パラメータの不揮発保存。
//
// 実機で詰めた値は電源を切ると消える、では調整が進まない。Flash最終ページに
// 1レコードだけ置き、起動時に読み戻す。書き換えはページ消去を伴いCPUが数十ms
// 止まるため、SAFE中に、変更が落ち着いてからまとめて1回だけ行う。
namespace param_store {

// 保存値を読み戻す。レコードが無い、または壊れていれば既定値のまま false。
bool load(domain::Parameters& parameters);

// 現在値を書き込む。内容が保存済みと同じなら何もせず true を返す。
bool save(const domain::Parameters& parameters);

// レコードを消す。次の起動は既定値になる。
bool clear();

// Flashに有効なレコードがあるか。
bool present();

}  // namespace param_store

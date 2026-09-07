#include "doctest.h"
#include "sts3215.hpp"
#include "servo_service.hpp"
#include <array>
#include <vector>
#include <cstring>
namespace {
DMA_HandleTypeDef dma;
UART_HandleTypeDef uart{&dma};
uint8_t* rx=nullptr;
uint16_t head=0,size=0;
uint32_t ticks=0;
std::array<uint8_t,256> regs;
std::vector<std::vector<uint8_t>> sent;
bool silent=false, noise=false, ignore_write=false;
uint8_t fault=0;
void reset() { regs.fill(0);regs[56]=0x34;regs[57]=0x08;sent.clear();ticks=0;silent=noise=ignore_write=false;fault=0;uart.ErrorCode=0; }
void append(const std::vector<uint8_t>& bytes) { for(auto b:bytes) {rx[head]=b;head=(head+1)%size;} dma.count=size-head; }
void status(uint8_t id, const std::vector<uint8_t>& data, uint8_t error=0) {
 std::vector<uint8_t> p{255,255,id,static_cast<uint8_t>(data.size()+2),error};
 p.insert(p.end(),data.begin(),data.end());
 p.push_back(domain::sts3215::checksum(p.data()+2,p.size()-2));append(p);
}
}
uint32_t HAL_GetTick() { return ticks++/100; }
void HAL_Delay(uint32_t ms) { ticks += ms * 100; }
HAL_StatusTypeDef HAL_UART_Receive_DMA(UART_HandleTypeDef*,uint8_t* data,uint16_t n) {rx=data;size=n;head=0;dma.count=n;return HAL_OK;}
HAL_StatusTypeDef HAL_UART_AbortReceive(UART_HandleTypeDef*) {uart.ErrorCode=0;return HAL_OK;}
HAL_StatusTypeDef HAL_UART_Receive(UART_HandleTypeDef*,uint8_t*,uint16_t,uint32_t) {return HAL_TIMEOUT;}
HAL_StatusTypeDef HAL_UART_Transmit(UART_HandleTypeDef*,uint8_t* p,uint16_t n,uint32_t) {
 sent.emplace_back(p,p+n);
 if(silent) return HAL_OK;
 if(p[4]==0x83) {
  if(!ignore_write) std::memcpy(regs.data()+p[5],p+8,p[6]);
 } else if(p[4]==3) {
  if(!ignore_write) std::memcpy(regs.data()+p[5],p+6,n-7);
  if(p[2]!=254)status(p[2],{},fault);
 } else if(p[4]==2) {
  if(noise) {append({0,7,255});status(9,{});status(p[2],{});noise=false;}
  status(p[2],std::vector<uint8_t>(regs.begin()+p[5],regs.begin()+p[5]+p[6]),fault);
 } else if(p[4]==1)status(p[2],{},fault);
 return HAL_OK;
}
TEST_CASE("循環DMAで位置応答を読み古いACKと他IDを読み飛ばす") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 for(unsigned i=0;i<50;++i) {noise=true;uint16_t position=0;REQUIRE(bus.readPosition(1,position)==Sts3215::Result::Ok);CHECK(position==2100);}
}
TEST_CASE("応答なしや読戻し不一致を送信成功として扱わない") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 silent=true;CHECK(bus.setTorque(1,true)==Sts3215::Result::Timeout);
 silent=false;ignore_write=true;CHECK(bus.setTorque(1,true)==Sts3215::Result::ReadbackMismatch);
 ignore_write=false;CHECK(bus.setTorque(1,true)==Sts3215::Result::Ok);
 CHECK(regs[40]==1);CHECK(sent[sent.size()-2][4]==0x83);
}
TEST_CASE("現在位置の目標を書いてからトルクを有効化する") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 REQUIRE(bus.preparePosition(1)==Sts3215::Result::Ok);
 CHECK(regs[42]==0x34);CHECK(regs[43]==0x08);CHECK(regs[40]==1);
 unsigned target=999,enable=999;
 for(unsigned i=0;i<sent.size();++i) if(sent[i][4]==0x83) {
  if(sent[i][5]==41) target=i;
  if(sent[i][5]==40&&sent[i][8]==1)enable=i;
 }
 CHECK(target<enable);
}
TEST_CASE("速度モードやサーボ異常では位置操作を開始しない") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 regs[33]=1;CHECK(bus.preparePosition(1)==Sts3215::Result::UnsupportedMode);CHECK(regs[40]==0);
 regs[33]=0;fault=4;CHECK(bus.preparePosition(1)==Sts3215::Result::ServoError);CHECK(bus.lastServoError()==4);
 fault=0;uint16_t pos=0;CHECK(bus.readPosition(1,pos)==Sts3215::Result::Ok);CHECK(bus.lastServoError()==0);
 regs[57]=0x80;CHECK(bus.readPosition(1,pos)==Sts3215::Result::ProtocolError);
}
TEST_CASE("目標の符号化が公式SDKの7byte指令と一致する") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 REQUIRE(bus.setTarget({1,50,2048,0,500})==Sts3215::Result::Ok);
 const std::vector<uint8_t> expected{255,255,254,12,131,41,7,1,50,0,8,0,0,244,1,18};
 CHECK(sent[0]==expected);
}

namespace {
std::vector<std::array<uint8_t,8>> replies;
void emitService(uint16_t, const uint8_t* data) { std::array<uint8_t,8> p{};std::memcpy(p.data(),data,8);replies.push_back(p); }
bool changeBaud(uint32_t) { return true; }
}
TEST_CASE("相対ステップのEXECUTE再送で二度動かさず停止で待機目標を破棄する") {
 reset();replies.clear();regs[33]=3;
 Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ServoService service(bus,emitService,changeBaud);
 const uint8_t stage[8]={1,22,1,20,0x80,100,0,100};
 const uint8_t execute[8]={1,23,1,7,0,0,0,0};
 service.handle(stage,true);service.handle(execute,true);
 REQUIRE(replies.back()[4]==0);
 const auto transmissions=sent.size();
 service.handle(execute,true);CHECK(sent.size()==transmissions);
 REQUIRE(service.stop());CHECK(regs[40]==0);CHECK_FALSE(service.active());
 const uint8_t empty[8]={1,23,1,8,0,0,0,0};
 service.handle(empty,true);CHECK(replies.back()[4]!=0);
}
TEST_CASE("保存設定は停止中だけ変更しモード3の角度制限とロックを読戻す") {
 reset();replies.clear();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ServoService service(bus,emitService,changeBaud);
 const uint8_t mode[8]={1,21,1,3,33,1,0,3};
 service.handle(mode,true);CHECK(replies.back()[4]!=0);CHECK(regs[33]==0);
 regs[11]=255;regs[12]=15;
 service.handle(mode,false);REQUIRE(replies.back()[4]==0);
 CHECK(regs[33]==3);CHECK(regs[11]==0);CHECK(regs[12]==0);CHECK(regs[55]==1);CHECK(regs[40]==0);
 const uint8_t arbitrary[8]={1,21,1,4,40,1,0,1};
 service.handle(arbitrary,false);CHECK(replies.back()[4]!=0);CHECK(regs[40]==0);
}

TEST_CASE("多回転位置は角度上下限0のときだけ許可し符号付き現在位置から開始する") {
 reset();replies.clear();regs[56]=0x88;regs[57]=0x13;
 Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ServoService service(bus,emitService,changeBaud);
 const uint8_t stage[8]={1,22,1,20,0x93,0x88,0,100};
 const uint8_t execute[8]={1,23,1,7,0,0,0,0};
 regs[11]=255;regs[12]=15;
 service.handle(stage,true);service.handle(execute,true);CHECK(replies.back()[4]!=0);CHECK(regs[40]==0);
 regs[11]=0;regs[12]=0;
 service.handle(stage,true);service.handle(execute,true);REQUIRE(replies.back()[4]==0);
 CHECK(regs[42]==0x88);CHECK(regs[43]==0x93);
 REQUIRE(service.stop());
}

TEST_CASE("現在位置が設定角度範囲外ならトルクを有効化しない") {
 reset();replies.clear();regs[11]=0xe8;regs[12]=3;
 Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ServoService service(bus,emitService,changeBaud);
 const uint8_t stage[8]={1,22,1,20,1,244,0,100};
 const uint8_t execute[8]={1,23,1,7,0,0,0,0};
 service.handle(stage,true);service.handle(execute,true);
 CHECK(replies.back()[4]!=0);CHECK(regs[40]==0);
 for(const auto& p:sent) CHECK_FALSE((p[4]==0x83 && p[5]==40 && p[8]==1));
}

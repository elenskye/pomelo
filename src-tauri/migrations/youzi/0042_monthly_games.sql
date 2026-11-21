-- 玩物有志 (v1.0 Phase 5): 每月回顾 also mentions the games finished. Only the ready-made prompt as it was written
-- (one the user changed stays theirs).
UPDATE schedules
   SET prompt = '回顾我上个月：收支和总资产的变化、读完看完的书和影视、通关的游戏、画了多少。分几块写，简洁；和再上一个月比一比。'
 WHERE builtin = 'monthly' AND prompt = '回顾我上个月：收支和总资产的变化、读完看完的书和影视、画了多少。分几块写，简洁；和再上一个月比一比。';

import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import type { UsageGroup } from '../api/generated/usage';
import { ALL_USAGE, reported, unavailable } from '../api/transports/mock/fixtures';
import { compareDecimalDescending, decimalBarPercent } from '../components/format';
import { ModelDistribution, partitionModels } from './ModelDistribution';

const group = (id: string, tokens: string | null, cost: string | null): UsageGroup => ({ id, usage: {
  tokens: {total:tokens === null ? unavailable() : reported(tokens),inputUncached:unavailable(),cacheRead:unavailable(),cacheWrite:unavailable(),outputTotal:unavailable(),outputReasoning:unavailable()},
  cost: { ...ALL_USAGE.cost, amountUsd: cost === null ? unavailable() : reported(cost, 'estimated') },
} });
describe('descending model distribution', () => {
  it('orders large tokens exactly and keeps unavailable separate from zero', () => {
    const models = [group('zero','0','0'),group('small','20','0.5'),group('unknown',null,'1.87'),group('large','9007199254740993',null),group('near','9007199254740992','111.49')];
    const sorted=partitionModels(models,'tokens');
    expect(sorted.ranked.map(model=>model.id)).toEqual(['large','near','small']);
    expect(sorted.unavailable.map(model=>model.id)).toEqual(['unknown']);
    expect(sorted.unused.map(model=>model.id)).toEqual(['zero']);
    expect(models.map(model=>model.id)).toEqual(['zero','small','unknown','large','near']);
  });
  it('sorts decimal costs descending without floating point rounding or invented prices', () => {
    const sorted=partitionModels([group('gpt','999',null),group('gemini',null,'111.49'),group('cheap','1','0.0000000000000000002'),group('cheaper','2','0.0000000000000000001')],'cost');
    expect(sorted.ranked.map(model=>model.id)).toEqual(['gemini','cheap','cheaper']);
    expect(sorted.unavailable.map(model=>model.id)).toEqual(['gpt']);
    expect(compareDecimalDescending('9007199254740993.01','9007199254740993.00')).toBe(-1);
    expect(compareDecimalDescending('1.20','1.2')).toBe(0);
    expect(decimalBarPercent('0.0000000000000000001','0.0000000000000000002')).toBe(50);
  });
  it('renders unused and missing details collapsed behind ranked models', () => {
    const html=renderToStaticMarkup(<ModelDistribution groups={[group('zero','0','0'),group('unavailable',null,'0.5'),group('used','120','2')]} />);
    expect(html).toContain('模型排序');expect(html).toContain('按 token');expect(html).toContain('按估算成本');
    expect(html.indexOf('>used<')).toBeLessThan(html.indexOf('>unavailable<'));
    expect(html.indexOf('>used<')).toBeLessThan(html.indexOf('>zero<'));
    expect(html).toContain('本范围未使用');expect(html).toContain('token明细不足');
    expect(html).not.toMatch(/<details[^>]*open/);
  });
});

import { Condition, ConditionResolver, PrintItem } from "../../types";

export interface RepeatableCondition {
    /** Name for debugging purposes. */
    name: string;
    originalCondition: Condition;
    condition: ConditionResolver | Condition;
    true?: PrintItem[];
    false?: PrintItem[];
}

/** Cache for creating repeatable conditions. */
export class RepeatableConditionCache {
    private readonly repeatableConditions = new Map<Condition, RepeatableCondition>();

    getOrCreate(condition: Condition) {
        let repeatableCondition = this.repeatableConditions.get(condition);

        if (repeatableCondition == null) {
            repeatableCondition = this.create(condition);
            this.repeatableConditions.set(condition, repeatableCondition);
        }

        return repeatableCondition;
    }

    private create(condition: Condition): RepeatableCondition {
        return {
            name: condition.name,
            originalCondition: condition,
            condition: condition.condition,
            true: condition.true && Array.from(condition.true),
            false: condition.false && Array.from(condition.false)
        };
    }
}

<template>
  <section class="text-style-editor">
    <h2>{{ title }}</h2>

    <div class="style-group">
      <h3>基础填充</h3>
      <div class="control-grid">
        <label class="form-field">
          <span>填充模式</span>
          <select v-model="paint.mode" @change="notifyChange">
            <option value="solid">纯色</option>
            <option value="gradient">线性渐变</option>
          </select>
        </label>
        <label class="form-field">
          <span>颜色</span>
          <input v-model="paint.color" type="color" @input="notifyChange" />
        </label>
        <label v-if="paint.mode === 'gradient'" class="form-field">
          <span>结束色</span>
          <input v-model="paint.colorEnd" type="color" @input="notifyChange" />
        </label>
        <label v-if="paint.mode === 'gradient'" class="form-field">
          <span>角度</span>
          <input v-model.number="paint.angle" type="number" min="0" max="360" @input="notifyChange" />
        </label>
      </div>
    </div>

    <div class="style-group">
      <label class="effect-toggle">
        <input v-model="paint.strokeEnabled" type="checkbox" @change="notifyChange" />
        <span>启用描边</span>
      </label>
      <div v-if="paint.strokeEnabled" class="control-grid effect-controls">
        <label class="form-field">
          <span>描边宽度</span>
          <input v-model.number="paint.strokeWidth" type="number" min="0.5" max="8" step="0.5" @input="notifyChange" />
        </label>
        <label class="form-field">
          <span>描边色</span>
          <input v-model="paint.strokeColor" type="color" @input="notifyChange" />
        </label>
      </div>
    </div>

    <div class="style-group">
      <label class="effect-toggle">
        <input v-model="paint.shadowEnabled" type="checkbox" @change="notifyChange" />
        <span>启用阴影</span>
      </label>
      <div v-if="paint.shadowEnabled" class="control-grid effect-controls">
        <label class="form-field">
          <span>阴影 X</span>
          <input v-model.number="paint.shadowX" type="number" min="-30" max="30" @input="notifyChange" />
        </label>
        <label class="form-field">
          <span>阴影 Y</span>
          <input v-model.number="paint.shadowY" type="number" min="-30" max="30" @input="notifyChange" />
        </label>
        <label class="form-field">
          <span>模糊度</span>
          <input v-model.number="paint.shadowBlur" type="number" min="0" max="40" @input="notifyChange" />
        </label>
        <label class="form-field">
          <span>阴影色</span>
          <input v-model="paint.shadowColor" type="text" @input="notifyChange" />
        </label>
      </div>
    </div>

    <div class="style-group">
      <label class="effect-toggle">
        <input v-model="paint.glowEnabled" type="checkbox" @change="notifyChange" />
        <span>启用发光</span>
      </label>
      <div v-if="paint.glowEnabled" class="control-grid effect-controls">
        <label class="form-field">
          <span>发光模糊度</span>
          <input v-model.number="paint.glowBlur" type="number" min="0" max="50" @input="notifyChange" />
        </label>
        <label class="form-field">
          <span>发光色</span>
          <input v-model="paint.glowColor" type="text" @input="notifyChange" />
        </label>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import type { TextPaint } from '../theme';

defineProps<{
  title: string;
  paint: TextPaint;
}>();

const emit = defineEmits<{
  change: [];
}>();

function notifyChange() {
  emit('change');
}
</script>

<style scoped>
.text-style-editor {
  padding-bottom: 16px;
  margin-bottom: 16px;
  border-bottom: 1px solid #dfdfe4;
}

.text-style-editor h2 {
  margin: 0 0 12px;
  color: #4d4d55;
  font-size: 13px;
}

.style-group {
  margin-bottom: 16px;
}

.style-group:last-child {
  margin-bottom: 0;
}

.style-group h3 {
  margin: 0 0 8px;
  color: #666670;
  font-size: 11px;
  font-weight: 650;
}

.control-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px 12px;
}

.form-field {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  color: #7b7b84;
  font-size: 11px;
}

.form-field input,
.form-field select {
  box-sizing: border-box;
  width: 100%;
  height: 31px;
  min-width: 0;
  border: 1px solid #cfcfd5;
  border-radius: 4px;
  padding: 0 8px;
  background: #fff;
  color: #33333a;
  font: inherit;
}

.form-field input[type="color"] {
  padding: 3px;
}

.effect-toggle {
  box-sizing: border-box;
  width: 100%;
  min-height: 34px;
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid #dcdce2;
  border-radius: 5px;
  padding: 0 10px;
  background: #eeeef2;
  color: #505059;
  font-size: 12px;
  font-weight: 650;
  cursor: pointer;
}

.effect-toggle input {
  accent-color: #d13c86;
}

.effect-controls {
  margin-top: 10px;
}
</style>

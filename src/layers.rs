//! Toggles for the globe and the live feeds.

#[derive(Clone, Debug)]
pub struct Layers {
    pub labels: bool,
    pub graticule: bool,
    pub sun: bool,
    pub city_lights: bool,
    pub atmosphere: bool,
    pub stars: bool,
    pub cities: bool,
    pub elevation: bool,
    pub weather: bool,
    pub quakes: bool,
    pub faults: bool,
    pub volcanoes: bool,
    pub ocean: bool,
    pub reading: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            labels: true,
            graticule: true,
            sun: true,
            city_lights: true,
            atmosphere: true,
            stars: true,
            cities: true,
            elevation: true,
            weather: true,
            quakes: true,
            faults: true,
            volcanoes: true,
            ocean: true,
            reading: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameSettings {
    pub graticule: bool,
    pub minor_graticule: bool,
    pub sun: bool,
    pub city_lights: f32,
    pub atmosphere: bool,
    pub stars: bool,
    pub elevation: bool,
    pub weather: bool,
    pub quakes: bool,
    pub faults: bool,
    pub volcanoes: bool,
    pub ocean: bool,
}

impl Layers {
    pub fn frame(&self, height: f32) -> FrameSettings {
        FrameSettings {
            graticule: self.graticule,
            minor_graticule: self.graticule && height < 1.4,
            sun: self.sun,
            city_lights: if self.city_lights { 1.0 } else { 0.0 },
            atmosphere: self.atmosphere,
            stars: self.stars,
            elevation: self.elevation,
            weather: self.weather,
            quakes: self.quakes,
            faults: self.faults,
            volcanoes: self.volcanoes,
            ocean: self.ocean,
        }
    }
}
